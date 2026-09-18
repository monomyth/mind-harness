//! Headless Cyton ingest probe. Same BrainFlowBoard ingest thread as the GUI.
//! Usage: cyton-probe <port> [seconds]
//!    or: OPENBCI_LIVE_SERIAL=<port> cyton-probe [seconds]
fn main() {
    let port = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("OPENBCI_LIVE_SERIAL").ok().filter(|s| !s.is_empty()))
        .unwrap_or_else(|| {
            eprintln!("usage: cyton-probe <port> [seconds]");
            eprintln!("   or: OPENBCI_LIVE_SERIAL=<port> cyton-probe [seconds]");
            std::process::exit(2);
        });
    let secs: u64 = std::env::args()
        .nth(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(12);
    if let Err(e) = mind_harness::board::ingest::run_headless_cyton_probe(&port, secs) {
        eprintln!("cyton-probe: {e}");
        std::process::exit(1);
    }
}
