//! Dedicated BrainFlow ingest thread.
//!
//! `BoardShim` is not thread-safe. The GUI used to `prepare_session` on a
//! connection thread and then call `start_stream` / `get_board_data` on the
//! egui tick. After the first drain (~145 Cyton columns) later UI pulls
//! returned 0 while Python on the same stick held ~249 Hz.
//!
//! This thread owns every `BoardShim` call. The UI only takes a mailbox.

use crate::board::impedance::split_cyton_config_cmds;
use crate::board::BoardError;
use brainflow::board_shim::BoardShim;
use brainflow::brainflow_input_params::BrainFlowInputParamsBuilder;
use brainflow::{BoardIds, BrainFlowPresets};
use ndarray::Array2;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle, ThreadId};
use std::time::{Duration, Instant};

static INGEST_SESSION: AtomicU64 = AtomicU64::new(1);

pub const INGEST_PERIOD: Duration = Duration::from_millis(4);
pub const MAILBOX_CAP: usize = 45_000;

const PREPARE_TIMEOUT: Duration = Duration::from_secs(20);
const STREAM_TIMEOUT: Duration = Duration::from_secs(10);
const CONFIG_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct IngestMailbox {
    pub rows: Vec<Vec<f64>>,
    pub error: Option<String>,
    pub empty_pulls: u64,
    pub pulls: u64,
    pub last_pull_thread: Option<ThreadId>,
}

impl IngestMailbox {
    pub fn take_rows(&mut self) -> (Vec<Vec<f64>>, Option<String>) {
        let rows = std::mem::take(&mut self.rows);
        let err = self.error.take();
        (rows, err)
    }

    pub fn push_rows(&mut self, mut new_rows: Vec<Vec<f64>>) {
        if self.rows.len() + new_rows.len() > MAILBOX_CAP {
            let excess = self.rows.len() + new_rows.len() - MAILBOX_CAP;
            if excess >= self.rows.len() {
                self.rows.clear();
            } else {
                self.rows.drain(0..excess);
            }
        }
        self.rows.append(&mut new_rows);
    }
}

/// Pump `pull` until Shutdown or the command channel disconnects.
/// Used by tests (fake sources) and as the inner cadence of the shim worker.
pub fn run_pull_loop<F>(cmds: Receiver<PullCmd>, mailbox: Arc<Mutex<IngestMailbox>>, mut pull: F)
where
    F: FnMut() -> Result<Vec<Vec<f64>>, String>,
{
    let tid = thread::current().id();
    loop {
        let wait_t0 = Instant::now();
        match cmds.recv_timeout(INGEST_PERIOD) {
            Ok(PullCmd::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        let wait = wait_t0.elapsed();
        let busy_t0 = Instant::now();
        apply_pull_result(tid, &mailbox, pull());
        crate::starve::SPLIT.add_ingest(wait + busy_t0.elapsed(), busy_t0.elapsed());
    }
}

#[derive(Debug)]
pub enum PullCmd {
    Shutdown,
}

pub fn apply_pull_result(
    tid: ThreadId,
    mailbox: &Mutex<IngestMailbox>,
    result: Result<Vec<Vec<f64>>, String>,
) {
    let Ok(mut mb) = mailbox.lock() else {
        return;
    };
    mb.pulls += 1;
    mb.last_pull_thread = Some(tid);
    match result {
        Ok(rows) => {
            if rows.is_empty() {
                mb.empty_pulls += 1;
            } else {
                mb.push_rows(rows);
            }
        }
        Err(e) => {
            mb.error = Some(e);
        }
    }
}

pub fn board_data_to_rows(arr: &Array2<f64>) -> Vec<Vec<f64>> {
    let n_chans = arr.nrows();
    let n_samples = arr.ncols();
    let mut rows = Vec::with_capacity(n_samples);
    for s in 0..n_samples {
        let mut row = Vec::with_capacity(n_chans);
        for c in 0..n_chans {
            row.push(arr[[c, s]]);
        }
        rows.push(row);
    }
    rows
}

enum ShimCmd {
    Prepare {
        board_id: BoardIds,
        serial_port: Option<String>,
        device_id: Option<String>,
        ip_address: Option<String>,
        ip_port: Option<usize>,
        reply: Sender<Result<(), String>>,
    },
    Start {
        buffer: usize,
        reply: Sender<Result<(), String>>,
    },
    Stop {
        reply: Sender<Result<(), String>>,
    },
    Config {
        cmd: String,
        reply: Sender<Result<(), String>>,
    },
    ConfigResponse {
        cmd: String,
        reply: Sender<Result<String, String>>,
    },
    Shutdown,
}

pub struct ShimIngest {
    tx: Mutex<Sender<ShimCmd>>,
    join: Mutex<Option<JoinHandle<()>>>,
    mailbox: Arc<Mutex<IngestMailbox>>,
    cmds_sent: AtomicU64,
}

impl ShimIngest {
    pub fn spawn() -> Self {
        let (tx, rx) = mpsc::channel();
        let mailbox = Arc::new(Mutex::new(IngestMailbox::default()));
        let mb = Arc::clone(&mailbox);
        let join = thread::Builder::new()
            .name("mind-harness-ingest".into())
            .spawn(move || shim_worker(rx, mb))
            .expect("spawn mind-harness-ingest");
        Self {
            tx: Mutex::new(tx),
            join: Mutex::new(Some(join)),
            mailbox,
            cmds_sent: AtomicU64::new(0),
        }
    }

    pub fn prepare(
        &self,
        board_id: BoardIds,
        serial_port: Option<String>,
        device_id: Option<String>,
        ip_address: Option<String>,
        ip_port: Option<usize>,
    ) -> Result<(), BoardError> {
        self.rpc(PREPARE_TIMEOUT, |reply| ShimCmd::Prepare {
            board_id,
            serial_port,
            device_id,
            ip_address,
            ip_port,
            reply,
        })
    }

    pub fn start_stream(&self, buffer: usize) -> Result<(), BoardError> {
        self.rpc(STREAM_TIMEOUT, |reply| ShimCmd::Start { buffer, reply })
    }

    pub fn stop_stream(&self) -> Result<(), BoardError> {
        self.rpc(STREAM_TIMEOUT, |reply| ShimCmd::Stop { reply })
    }

    pub fn config(&self, cmd: &str) -> Result<(), BoardError> {
        self.rpc(CONFIG_TIMEOUT, |reply| ShimCmd::Config {
            cmd: cmd.to_string(),
            reply,
        })
    }

    /// Like `config`, but returns the board's reply string (needed for SD confirm).
    pub fn config_response(&self, cmd: &str) -> Result<String, BoardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .lock()
            .map_err(|_| BoardError::Io("ingest command lock".into()))?
            .send(ShimCmd::ConfigResponse {
                cmd: cmd.to_string(),
                reply: reply_tx,
            })
            .map_err(|_| BoardError::Io("ingest thread died".into()))?;
        self.cmds_sent.fetch_add(1, Ordering::Relaxed);
        reply_rx
            .recv_timeout(CONFIG_TIMEOUT)
            .map_err(|_| BoardError::Io("ingest command timeout".into()))?
            .map_err(BoardError::BrainFlow)
    }

    pub fn take_rows(&self) -> (Vec<Vec<f64>>, Option<String>) {
        match self.mailbox.lock() {
            Ok(mut mb) => mb.take_rows(),
            Err(p) => p.into_inner().take_rows(),
        }
    }

    pub fn last_pull_thread(&self) -> Option<ThreadId> {
        let mb = self.mailbox.lock().unwrap_or_else(|p| p.into_inner());
        mb.last_pull_thread
    }

    pub fn cmds_sent(&self) -> u64 {
        self.cmds_sent.load(Ordering::Relaxed)
    }

    pub fn shutdown(self) {
        {
            let tx = self.tx.lock().unwrap_or_else(|p| p.into_inner());
            let _ = tx.send(ShimCmd::Shutdown);
        }
        if let Some(join) = self.join.lock().unwrap_or_else(|p| p.into_inner()).take() {
            let _ = join.join();
        }
    }

    fn rpc(
        &self,
        timeout: Duration,
        make: impl FnOnce(Sender<Result<(), String>>) -> ShimCmd,
    ) -> Result<(), BoardError> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.tx
            .lock()
            .map_err(|_| BoardError::Io("ingest command lock".into()))?
            .send(make(reply_tx))
            .map_err(|_| BoardError::Io("ingest thread died".into()))?;
        self.cmds_sent.fetch_add(1, Ordering::Relaxed);
        reply_rx
            .recv_timeout(timeout)
            .map_err(|_| BoardError::Io("ingest command timeout".into()))?
            .map_err(BoardError::BrainFlow)
    }
}

fn build_params(
    serial_port: Option<String>,
    device_id: Option<String>,
    ip_address: Option<String>,
    ip_port: Option<usize>,
) -> brainflow::brainflow_input_params::BrainFlowInputParams {
    let mut builder = BrainFlowInputParamsBuilder::default();
    if let Some(port) = serial_port {
        builder = builder.serial_port(port).timeout(15);
    }
    if let Some(dev) = device_id {
        builder = builder.serial_number(dev);
    }
    if let Some(ip) = ip_address {
        builder = builder.ip_address(ip);
    }
    if let Some(port) = ip_port {
        builder = builder.ip_port(port);
    }
    // BrainFlow keys sessions by (board_id, params). Parallel Synthetic tests
    // share empty params and hit ANOTHER_BOARD_IS_CREATED; a unique other_info
    // keeps each ingest thread on its own C++ session.
    let id = INGEST_SESSION.fetch_add(1, Ordering::Relaxed);
    builder = builder.other_info(format!("mh-ingest-{id}"));
    builder.build()
}

fn shim_worker(cmds: Receiver<ShimCmd>, mailbox: Arc<Mutex<IngestMailbox>>) {
    let tid = thread::current().id();
    let mut shim: Option<BoardShim> = None;
    let mut streaming = false;
    loop {
        match cmds.recv_timeout(INGEST_PERIOD) {
            Ok(cmd) => {
                if handle_shim_cmd(cmd, &mut shim, &mut streaming) {
                    break;
                }
            }
            Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }
        while let Ok(cmd) = cmds.try_recv() {
            if handle_shim_cmd(cmd, &mut shim, &mut streaming) {
                return;
            }
        }
        if !streaming {
            continue;
        }
        let Some(s) = shim.as_ref() else {
            continue;
        };
        // Skip the rust binding's 0-column get_board_data (dangling Vec ptr).
        let wait = INGEST_PERIOD;
        let busy_t0 = Instant::now();
        let result = match s.get_board_data_count(BrainFlowPresets::DefaultPreset) {
            Ok(0) => Ok(Vec::new()),
            Ok(_) => match s.get_board_data(None, BrainFlowPresets::DefaultPreset) {
                Ok(arr) => Ok(board_data_to_rows(&arr)),
                Err(e) => Err(format!("{e:?}")),
            },
            Err(e) => Err(format!("{e:?}")),
        };
        apply_pull_result(tid, &mailbox, result);
        crate::starve::SPLIT.add_ingest(wait + busy_t0.elapsed(), busy_t0.elapsed());
    }
    if let Some(s) = shim.take() {
        if streaming {
            let _ = s.stop_stream();
        }
        let _ = s.release_session();
    }
}

/// Returns true if the worker should exit.
fn handle_shim_cmd(
    cmd: ShimCmd,
    shim: &mut Option<BoardShim>,
    streaming: &mut bool,
) -> bool {
    match cmd {
        ShimCmd::Shutdown => true,
        ShimCmd::Prepare {
            board_id,
            serial_port,
            device_id,
            ip_address,
            ip_port,
            reply,
        } => {
            if shim.is_some() {
                let _ = reply.send(Ok(()));
                return false;
            }
            let params = build_params(serial_port, device_id, ip_address, ip_port);
            let res = BoardShim::new(board_id, params)
                .map_err(|e| format!("{e:?}"))
                .and_then(|s| {
                    s.prepare_session()
                        .map_err(|e| format!("{e:?}"))
                        .map(|()| s)
                });
            match res {
                Ok(s) => {
                    *shim = Some(s);
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(e));
                }
            }
            false
        }
        ShimCmd::Start { buffer, reply } => {
            let Some(s) = shim.as_ref() else {
                let _ = reply.send(Err("board is not initialized".into()));
                return false;
            };
            if *streaming {
                let _ = reply.send(Ok(()));
                return false;
            }
            match s.start_stream(buffer, "") {
                Ok(()) => {
                    *streaming = true;
                    let _ = reply.send(Ok(()));
                }
                Err(e) => {
                    let _ = reply.send(Err(format!("{e:?}")));
                }
            }
            false
        }
        ShimCmd::Stop { reply } => {
            if let Some(s) = shim.as_ref() {
                if *streaming {
                    if let Err(e) = s.stop_stream() {
                        *streaming = false;
                        let _ = reply.send(Err(format!("{e:?}")));
                        return false;
                    }
                    *streaming = false;
                }
            }
            let _ = reply.send(Ok(()));
            false
        }
        ShimCmd::Config { cmd, reply } => {
            let Some(s) = shim.as_ref() else {
                let _ = reply.send(Err("board is not initialized".into()));
                return false;
            };
            let mut err = None;
            for piece in split_cyton_config_cmds(&cmd) {
                if let Err(e) = s.config_board(piece) {
                    err = Some(format!("{e:?}"));
                    break;
                }
            }
            let _ = reply.send(err.map(Err).unwrap_or(Ok(())));
            false
        }
        ShimCmd::ConfigResponse { cmd, reply } => {
            let Some(s) = shim.as_ref() else {
                let _ = reply.send(Err("board is not initialized".into()));
                return false;
            };
            let mut last_ok = String::new();
            let mut err = None;
            for piece in split_cyton_config_cmds(&cmd) {
                match s.config_board(piece) {
                    Ok(resp) => last_ok = resp,
                    Err(e) => {
                        err = Some(format!("{e:?}"));
                        break;
                    }
                }
            }
            let _ = reply.send(err.map(Err).unwrap_or(Ok(last_ok)));
            false
        }
    }
}

/// Headless 16 ms drain of the same `BrainFlowBoard` path the GUI uses.
pub fn run_headless_cyton_probe(port: &str, secs: u64) -> Result<(), BoardError> {
    use crate::board::brainflow_board::BrainFlowBoard;
    use crate::board::DataSource;
    use crate::starve::StarveLog;
    use std::time::Instant;

    let mut board = BrainFlowBoard::cyton_serial(port);
    eprintln!("prepare {port} (ingest thread)");
    board.initialize()?;
    board.start_streaming()?;
    let mut starve = StarveLog::default();
    starve.note_window_start();
    let t0 = Instant::now();
    let mut last = t0;
    let mut window_del = 0u64;
    let mut window_lost = 0u64;
    let mut window_empty = 0u64;
    let mut last_err = None;
    let nominal = board.sample_rate() as f64;
    while t0.elapsed().as_secs() < secs {
        thread::sleep(Duration::from_millis(16));
        board.update();
        let d = board.recent_samples_delivered() as u64;
        let lost = board.recent_samples_lost() as u64;
        if d == 0 {
            window_empty += 1;
        } else {
            window_del += d;
            window_lost += lost;
        }
        if let Some(e) = board.last_ingest_error() {
            last_err = Some(e);
        }
        if last.elapsed().as_secs_f64() >= 1.0 {
            let dt = last.elapsed().as_secs_f64();
            starve.tick(
                window_del,
                window_lost,
                dt,
                window_empty,
                nominal,
                None,
                last_err.clone(),
            );
            println!(
                "t={:.1} delivered={} wall_hz={:.1} lost={} empty_ticks={} err={:?}",
                t0.elapsed().as_secs_f64(),
                window_del,
                window_del as f64 / dt,
                window_lost,
                window_empty,
                last_err
            );
            window_del = 0;
            window_lost = 0;
            window_empty = 0;
            last = Instant::now();
        }
    }
    let _ = board.stop_streaming();
    let _ = board.uninitialize();
    println!("done wall={:.2}s", t0.elapsed().as_secs_f64());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Instant;

    /// Live Cyton on the UI thread: first `get_board_data` returns ~145 columns,
    /// later calls return 0. A dedicated pull thread must keep filling.
    struct UiBurstThenSilence {
        ui_thread: ThreadId,
        pulls_on_ui: AtomicU64,
    }

    impl UiBurstThenSilence {
        fn pull(&self) -> Result<Vec<Vec<f64>>, String> {
            if thread::current().id() == self.ui_thread {
                let n = self.pulls_on_ui.fetch_add(1, Ordering::SeqCst);
                if n == 0 {
                    Ok(vec![vec![1.0]; 145])
                } else {
                    Ok(Vec::new())
                }
            } else {
                Ok(vec![vec![1.0]; 1])
            }
        }
    }

    #[test]
    fn ui_thread_pull_dies_after_first_burst() {
        let src = UiBurstThenSilence {
            ui_thread: thread::current().id(),
            pulls_on_ui: AtomicU64::new(0),
        };
        let first = src.pull().unwrap();
        let second = src.pull().unwrap();
        assert_eq!(first.len(), 145);
        assert!(second.is_empty(), "UI-thread pull must reproduce the starve");
    }

    #[test]
    fn dedicated_pull_loop_does_not_die_after_first_burst() {
        let src = Arc::new(UiBurstThenSilence {
            ui_thread: thread::current().id(),
            pulls_on_ui: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::channel();
        let mailbox = Arc::new(Mutex::new(IngestMailbox::default()));
        let mb = Arc::clone(&mailbox);
        let src_thread = Arc::clone(&src);
        let handle = thread::spawn(move || {
            run_pull_loop(rx, mb, move || src_thread.pull());
        });
        thread::sleep(Duration::from_millis(40));
        let (rows, pulls, tid) = {
            let mut g = mailbox.lock().unwrap();
            let rows = std::mem::take(&mut g.rows);
            (rows, g.pulls, g.last_pull_thread)
        };
        let _ = tx.send(PullCmd::Shutdown);
        let _ = handle.join();
        assert!(pulls >= 2, "ingest thread must pull more than once, pulls={pulls}");
        assert!(
            rows.len() >= 2,
            "dedicated thread must keep filling after first pull, n={}",
            rows.len()
        );
        assert_ne!(tid, Some(thread::current().id()));
        assert_eq!(src.pulls_on_ui.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn mailbox_accumulates_when_ui_does_not_drain() {
        let produced = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel();
        let mailbox = Arc::new(Mutex::new(IngestMailbox::default()));
        let mb = Arc::clone(&mailbox);
        let prod = Arc::clone(&produced);
        let handle = thread::spawn(move || {
            run_pull_loop(rx, mb, move || {
                prod.fetch_add(1, Ordering::SeqCst);
                Ok(vec![vec![1.0]])
            });
        });
        thread::sleep(Duration::from_millis(50));
        let n = mailbox.lock().unwrap().rows.len();
        let _ = tx.send(PullCmd::Shutdown);
        let _ = handle.join();
        assert!(
            n >= 5,
            "50ms at 4ms period without drain should queue several rows, n={n}"
        );
    }

    #[test]
    fn ui_cadence_drain_of_clocked_source_holds_rate() {
        struct Clock {
            t0: Instant,
            hz: f64,
            emitted: AtomicU64,
        }
        impl Clock {
            fn pull(&self) -> Vec<Vec<f64>> {
                let should = (self.t0.elapsed().as_secs_f64() * self.hz) as u64;
                let prev = self.emitted.load(Ordering::SeqCst);
                let n = should.saturating_sub(prev);
                self.emitted.store(should, Ordering::SeqCst);
                vec![vec![0.0]; n as usize]
            }
        }
        let clock = Arc::new(Clock {
            t0: Instant::now(),
            hz: 250.0,
            emitted: AtomicU64::new(0),
        });
        let (tx, rx) = mpsc::channel();
        let mailbox = Arc::new(Mutex::new(IngestMailbox::default()));
        let mb = Arc::clone(&mailbox);
        let c = Arc::clone(&clock);
        let handle = thread::spawn(move || {
            run_pull_loop(rx, mb, move || Ok(c.pull()));
        });
        let mut total = 0usize;
        let mut second_totals = Vec::new();
        let t0 = Instant::now();
        let mut last = t0;
        let mut window = 0usize;
        while t0.elapsed() < Duration::from_millis(1200) {
            thread::sleep(Duration::from_millis(16));
            let n = {
                let mut g = mailbox.lock().unwrap();
                std::mem::take(&mut g.rows).len()
            };
            total += n;
            window += n;
            if last.elapsed() >= Duration::from_millis(400) {
                second_totals.push(window);
                window = 0;
                last = Instant::now();
            }
        }
        let _ = tx.send(PullCmd::Shutdown);
        let _ = handle.join();
        assert!(
            total >= 200,
            "clocked 250 Hz source over ~1.2s, got {total} (windows={second_totals:?})"
        );
        assert!(
            second_totals.iter().all(|&n| n > 50),
            "no drained window should starve, windows={second_totals:?}"
        );
    }

    #[test]
    fn board_data_to_rows_is_column_major_samples() {
        let arr = Array2::from_shape_vec((2, 3), vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0]).unwrap();
        assert_eq!(
            board_data_to_rows(&arr),
            vec![vec![1.0, 4.0], vec![2.0, 5.0], vec![3.0, 6.0]]
        );
    }

    /// Record-start starve: if BDF/header/sidecar wait on the pull thread, the 4 ms
    /// loop misses BrainFlow and Hertz dies. A slow logger must not be that wait.
    #[test]
    fn ingest_holds_250hz_while_slow_logger_runs() {
        use crate::data_logger::{RecordPump, RecordingSample};

        let pump = RecordPump::spawn_with_write_delay(Duration::from_millis(8));
        let produced = Arc::new(AtomicU64::new(0));
        let (tx, rx) = mpsc::channel();
        let mailbox = Arc::new(Mutex::new(IngestMailbox::default()));
        let mb = Arc::clone(&mailbox);
        let prod = Arc::clone(&produced);
        let sample_tx = pump.sample_tx();
        let handle = thread::spawn(move || {
            run_pull_loop(rx, mb, move || {
                prod.fetch_add(1, Ordering::SeqCst);
                sample_tx.send(RecordingSample {
                    packet_index: 0.0,
                    exg: vec![0.0; 8],
                    accel: [0.0; 3],
                    ..Default::default()
                });
                Ok(vec![vec![1.0]])
            });
        });
        thread::sleep(Duration::from_millis(400));
        let n = produced.load(Ordering::SeqCst);
        let _ = tx.send(PullCmd::Shutdown);
        let _ = handle.join();
        drop(pump);
        assert!(
            n >= 70,
            "400ms at 4ms ingest period ≈ 100 pulls (250 Hz). Slow disk on the ingest thread yields ~40. got {n}"
        );
    }
}
