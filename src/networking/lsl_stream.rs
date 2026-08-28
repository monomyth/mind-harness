//! LSL (Lab Streaming Layer) output support.
//! Uses the `lsl` crate.

use lsl::{StreamInfo, StreamOutlet};

pub struct LslOutlet {
    outlet: StreamOutlet,
}

impl LslOutlet {
    pub fn new(stream_name: &str, channel_count: usize) -> Result<Self, String> {
        let info = StreamInfo::new(
            stream_name,
            "EEG",
            channel_count as i32,
            0.0, // irregular rate (we push when data arrives)
            lsl::ChannelFormat::Float32,
            "OpenBCI_Rust_GUI",
        );

        let outlet = StreamOutlet::new(&info, 0, 360).map_err(|e| e.to_string())?;
        Ok(Self { outlet })
    }

    pub fn push_sample(&mut self, sample: &[f64]) -> Result<(), String> {
        let sample_f32: Vec<f32> = sample.iter().map(|&x| x as f32).collect();
        self.outlet.push_sample(&sample_f32).map_err(|e| e.to_string())
    }
}