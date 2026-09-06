//! Mock brainflow crate for compilation on environments without the real brainflow library.

use ndarray::Array2;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NoiseTypes {
    Fifty,
    Sixty,
    #[default]
    FiftyAndSixty,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BoardIds {
    #[default]
    SyntheticBoard = -1,
    CytonBoard = 0,
    GanglionBoard = 1,
    CytonDaisyBoard = 2,
    GanglionNativeBoard = 3,
    GanglionWifiBoard = 4,
    CytonWifiBoard = 5,
    CytonDaisyWifiBoard = 6,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrainFlowPresets {
    #[default]
    DefaultPreset,
    AuxiliaryPreset,
    AncillaryPreset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FilterTypes {
    #[default]
    Butterworth,
    Chebyshev1,
    Bessel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrainFlowClassifiers {
    #[default]
    DefaultClassifier,
    DnnClassifier,
    Regression,
    OnnxClassifier,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BrainFlowMetrics {
    #[default]
    Mindfulness,
    Restfulness,
    UserDefined,
}

#[derive(Debug)]
pub struct BrainFlowError(pub String);

impl fmt::Display for BrainFlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "BrainFlowError: {}", self.0)
    }
}

impl std::error::Error for BrainFlowError {}

pub mod board_shim {
    use super::*;

    pub struct BoardShim {
        board_id: BoardIds,
    }

    impl BoardShim {
        pub fn new(
            board_id: BoardIds,
            _params: super::brainflow_input_params::BrainFlowInputParams,
        ) -> Result<Self, BrainFlowError> {
            Ok(Self { board_id })
        }

        pub fn prepare_session(&self) -> Result<(), BrainFlowError> {
            Ok(())
        }

        pub fn start_stream(&self, _buffer_size: usize, _streamer_params: &str) -> Result<(), BrainFlowError> {
            Ok(())
        }

        pub fn stop_stream(&self) -> Result<(), BrainFlowError> {
            Ok(())
        }

        pub fn release_session(&self) -> Result<(), BrainFlowError> {
            Ok(())
        }

        pub fn get_board_data_count(&self, _preset: BrainFlowPresets) -> Result<usize, BrainFlowError> {
            Ok(0)
        }

        pub fn get_board_data(
            &self,
            _num_samples: Option<usize>,
            _preset: BrainFlowPresets,
        ) -> Result<Array2<f64>, BrainFlowError> {
            Ok(Array2::zeros((0, 0)))
        }

        pub fn config_board(&self, _config: &str) -> Result<String, BrainFlowError> {
            Ok(String::new())
        }
    }

    pub fn get_sampling_rate(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<i32, BrainFlowError> {
        Ok(250)
    }

    pub fn get_exg_channels(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<Vec<usize>, BrainFlowError> {
        Ok(vec![1, 2, 3, 4, 5, 6, 7, 8])
    }

    pub fn get_accel_channels(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<Vec<usize>, BrainFlowError> {
        Ok(vec![9, 10, 11])
    }

    pub fn get_resistance_channels(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<Vec<usize>, BrainFlowError> {
        Ok(vec![])
    }

    pub fn get_analog_channels(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<Vec<usize>, BrainFlowError> {
        Ok(vec![])
    }

    pub fn get_other_channels(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<Vec<usize>, BrainFlowError> {
        Ok(vec![])
    }

    pub fn get_package_num_channel(
        _board_id: BoardIds,
        _preset: BrainFlowPresets,
    ) -> Result<usize, BrainFlowError> {
        Ok(0)
    }
}

pub mod brainflow_input_params {
    #[derive(Debug, Clone, Default)]
    pub struct BrainFlowInputParams {
        pub serial_port: Option<String>,
        pub ip_address: Option<String>,
        pub ip_port: Option<usize>,
        pub serial_number: Option<String>,
        pub timeout: Option<i32>,
        pub other_info: Option<String>,
    }

    #[derive(Debug, Clone, Default)]
    pub struct BrainFlowInputParamsBuilder {
        params: BrainFlowInputParams,
    }

    impl BrainFlowInputParamsBuilder {
        pub fn serial_port(mut self, port: String) -> Self {
            self.params.serial_port = Some(port);
            self
        }

        pub fn ip_address(mut self, addr: String) -> Self {
            self.params.ip_address = Some(addr);
            self
        }

        pub fn ip_port(mut self, port: usize) -> Self {
            self.params.ip_port = Some(port);
            self
        }

        pub fn serial_number(mut self, num: String) -> Self {
            self.params.serial_number = Some(num);
            self
        }

        pub fn timeout(mut self, timeout: i32) -> Self {
            self.params.timeout = Some(timeout);
            self
        }

        pub fn other_info(mut self, info: String) -> Self {
            self.params.other_info = Some(info);
            self
        }

        pub fn build(self) -> BrainFlowInputParams {
            self.params
        }
    }
}

pub mod data_filter {
    use super::*;

    pub fn perform_bandstop(
        _data: &mut [f64],
        _sampling_rate: usize,
        _start_freq: f64,
        _stop_freq: f64,
        _order: i32,
        _filter_type: FilterTypes,
        _ripple: f64,
    ) -> Result<(), BrainFlowError> {
        Ok(())
    }

    pub fn perform_bandpass(
        _data: &mut [f64],
        _sampling_rate: usize,
        _start_freq: f64,
        _stop_freq: f64,
        _order: i32,
        _filter_type: FilterTypes,
        _ripple: f64,
    ) -> Result<(), BrainFlowError> {
        Ok(())
    }

    pub fn get_avg_band_powers<T: Into<ndarray::Array2<f64>>, C: AsRef<[usize]>>(
        _data: T,
        _channels: C,
        _sampling_rate: usize,
        _apply_filters: bool,
    ) -> Result<(Vec<f64>, Vec<f64>), BrainFlowError> {
        Ok((vec![0.0; 5], vec![0.0; 5]))
    }
}

pub mod ml_model {
    use super::*;

    pub struct MlModel {
        _params: brainflow_model_params::BrainFlowModelParams,
    }

    impl MlModel {
        pub fn new(params: brainflow_model_params::BrainFlowModelParams) -> Result<Self, BrainFlowError> {
            Ok(Self { _params: params })
        }

        pub fn prepare(&self) -> Result<(), BrainFlowError> {
            Ok(())
        }

        pub fn predict(&self, _features: &[f64]) -> Result<Vec<f64>, BrainFlowError> {
            Ok(vec![0.5])
        }

        pub fn release(&self) -> Result<(), BrainFlowError> {
            Ok(())
        }
    }
}

pub mod brainflow_model_params {
    use super::*;

    #[derive(Debug, Clone, Default)]
    pub struct BrainFlowModelParams {
        pub metric: BrainFlowMetrics,
        pub classifier: BrainFlowClassifiers,
    }

    #[derive(Debug, Clone, Default)]
    pub struct BrainFlowModelParamsBuilder {
        params: BrainFlowModelParams,
    }

    impl BrainFlowModelParamsBuilder {
        pub fn new() -> Self {
            Self::default()
        }

        pub fn metric(mut self, metric: BrainFlowMetrics) -> Self {
            self.params.metric = metric;
            self
        }

        pub fn classifier(mut self, classifier: BrainFlowClassifiers) -> Self {
            self.params.classifier = classifier;
            self
        }

        pub fn file(self, _path: &str) -> Self {
            self
        }

        pub fn build(self) -> BrainFlowModelParams {
            self.params
        }
    }
}
