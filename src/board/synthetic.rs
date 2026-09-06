//! Real BrainFlow Synthetic Board implementation.
//!
//! This version uses the official `brainflow` Rust binding to create a
//! `BoardShim` with `BoardIds::SyntheticBoard`. This is the proper equivalent
//! of the Java `BoardBrainFlowSynthetic` class.

use crate::board::{BoardError, DataSource};
use brainflow::board_shim::{self, BoardShim};
use brainflow::brainflow_input_params::BrainFlowInputParamsBuilder;
use brainflow::{BoardIds, BrainFlowPresets};
use std::sync::Mutex;

/// A synthetic board powered by the real BrainFlow C++ engine.
/// Phase 7: retained for direct use / testing; current app uses the BrainFlowBoard facade
/// (which internally can delegate). Silenced to keep warning count at 0 for our crate.
#[allow(dead_code)]
pub struct SyntheticBoard {
    board: Option<BoardShim>,
    num_exg: usize,
    exg_channels: Vec<usize>,
    sample_rate: i32,
    is_streaming: bool,
    latest_data: Mutex<Vec<Vec<f64>>>,
}

impl SyntheticBoard {
    /// Phase 7: constructor for the low-level SyntheticBoard (currently unused in favor of
    /// BrainFlowBoard::synthetic wrapper for polymorphism). Kept for completeness.
    #[allow(dead_code)]
    pub fn new(num_exg_channels: usize) -> Self {
        Self {
            board: None,
            num_exg: num_exg_channels,
            exg_channels: (0..num_exg_channels).collect(),
            sample_rate: 250,
            is_streaming: false,
            latest_data: Mutex::new(Vec::new()),
        }
    }
}

impl DataSource for SyntheticBoard {
    fn initialize(&mut self) -> Result<(), BoardError> {
        if self.board.is_some() {
            return Ok(());
        }

        let params = BrainFlowInputParamsBuilder::default().build();
        let shim = BoardShim::new(BoardIds::SyntheticBoard, params)
            .map_err(|e| BoardError::BrainFlow(e.to_string()))?;

        shim.prepare_session()
            .map_err(|e| BoardError::BrainFlow(e.to_string()))?;

        // Query real metadata using free functions from the board_shim module
        if let Ok(sr) =
            board_shim::get_sampling_rate(BoardIds::SyntheticBoard, BrainFlowPresets::DefaultPreset)
        {
            self.sample_rate = sr as i32;
        }

        if let Ok(chans) =
            board_shim::get_exg_channels(BoardIds::SyntheticBoard, BrainFlowPresets::DefaultPreset)
        {
            self.exg_channels = chans.into_iter().collect();
            self.num_exg = self.exg_channels.len();
        }

        self.board = Some(shim);
        Ok(())
    }

    fn uninitialize(&mut self) -> Result<(), BoardError> {
        if let Some(shim) = self.board.take() {
            if self.is_streaming {
                let _ = shim.stop_stream();
            }
            let _ = shim.release_session();
        }
        self.is_streaming = false;
        Ok(())
    }

    fn update(&mut self) {
        if let Some(ref mut shim) = self.board {
            if self.is_streaming {
                // Get the latest data from BrainFlow (all channels, newest samples first in some versions)
                match shim.get_board_data(None, BrainFlowPresets::DefaultPreset) {
                    Ok(arr) => {
                        // arr is ndarray::Array2<f64> with shape (channels, samples)
                        // We want Vec<Vec<f64>> where outer vec = time samples
                        let n_chans = arr.nrows();
                        let n_samples = arr.ncols();

                        let mut new_samples = Vec::with_capacity(n_samples);

                        for s in 0..n_samples {
                            let mut row = Vec::with_capacity(n_chans);
                            for c in 0..n_chans {
                                row.push(arr[[c, s]]);
                            }
                            new_samples.push(row);
                        }

                        if let Ok(mut guard) = self.latest_data.lock() {
                            let max_keep = (self.sample_rate as usize * 6).max(2000);
                            if guard.len() + new_samples.len() > max_keep {
                                let excess = guard.len() + new_samples.len() - max_keep;
                                guard.drain(0..excess);
                            }
                            guard.extend(new_samples);
                        }
                    }
                    Err(e) => {
                        tracing::warn!("BrainFlow get_board_data error: {}", e);
                    }
                }
            }
        }
    }

    fn start_streaming(&mut self) -> Result<(), BoardError> {
        if let Some(ref mut shim) = self.board {
            if !self.is_streaming {
                shim.start_stream(45000, "")
                    .map_err(|e| BoardError::BrainFlow(e.to_string()))?;
                self.is_streaming = true;
            }
            Ok(())
        } else {
            Err(BoardError::NotInitialized)
        }
    }

    fn stop_streaming(&mut self) -> Result<(), BoardError> {
        if let Some(ref mut shim) = self.board {
            if self.is_streaming {
                let _ = shim.stop_stream();
                self.is_streaming = false;
            }
        }
        Ok(())
    }

    fn is_streaming(&self) -> bool {
        self.is_streaming
    }

    fn total_channel_count(&self) -> usize {
        // Synthetic board in BrainFlow typically has 12–16 total channels depending on config
        12
    }

    fn exg_channels(&self) -> &[usize] {
        &self.exg_channels
    }

    fn accel_channels(&self) -> &[usize] {
        &[] // Synthetic board in this implementation does not expose accel by default
    }

    fn sample_rate(&self) -> i32 {
        self.sample_rate
    }

    fn get_data(&self, max_samples: usize) -> Vec<Vec<f64>> {
        if let Ok(guard) = self.latest_data.lock() {
            let len = guard.len();
            let start = len.saturating_sub(max_samples);
            guard[start..].to_vec()
        } else {
            Vec::new()
        }
    }

    fn get_channel_data(&self, channel: usize, max_samples: usize) -> Vec<f64> {
        if let Ok(guard) = self.latest_data.lock() {
            let len = guard.len();
            let start = len.saturating_sub(max_samples);
            guard[start..]
                .iter()
                .map(|row| row.get(channel).copied().unwrap_or(0.0))
                .collect()
        } else {
            Vec::new()
        }
    }

    fn get_frame_data(&self) -> Vec<Vec<f64>> {
        if let Ok(guard) = self.latest_data.lock() {
            guard.last().cloned().into_iter().collect()
        } else {
            Vec::new()
        }
    }

    fn name(&self) -> &str {
        "BrainFlow Synthetic Board (real)"
    }
}
