//! LSL outlets (Java `W_Networking` names: `obci_eeg1` / type `EEG`, plus a marker stream).
//!
//! Linked against Homebrew `lsl.framework` when present (`cfg(has_liblsl)`).

#[cfg(has_liblsl)]
use std::ffi::CString;
#[cfg(has_liblsl)]
use std::os::raw::c_char;
#[cfg(all(has_liblsl, test))]
use std::ptr;

#[cfg(has_liblsl)]
#[allow(dead_code)]
mod ffi {
    use super::*;
    pub type StreamInfo = *mut std::ffi::c_void;
    pub type Outlet = *mut std::ffi::c_void;
    pub type Inlet = *mut std::ffi::c_void;

    pub const CFT_DOUBLE64: i32 = 2;
    pub const CFT_STRING: i32 = 3;

    #[link(name = "lsl", kind = "framework")]
    unsafe extern "C" {
        pub fn lsl_create_streaminfo(
            name: *const c_char,
            type_: *const c_char,
            channel_count: i32,
            nominal_srate: f64,
            channel_format: i32,
            source_id: *const c_char,
        ) -> StreamInfo;
        pub fn lsl_destroy_streaminfo(info: StreamInfo);
        pub fn lsl_create_outlet(info: StreamInfo, chunk_size: i32, max_buffered: i32) -> Outlet;
        pub fn lsl_destroy_outlet(out: Outlet);
        pub fn lsl_push_sample_d(out: Outlet, data: *const f64) -> i32;
        pub fn lsl_push_sample_str(out: Outlet, data: *const *const c_char) -> i32;
        pub fn lsl_resolve_byprop(
            buffer: *mut StreamInfo,
            buffer_elements: u32,
            prop: *const c_char,
            value: *const c_char,
            minimum: i32,
            timeout: f64,
        ) -> i32;
        pub fn lsl_create_inlet(
            info: StreamInfo,
            max_buflen: i32,
            max_chunklen: i32,
            recover: i32,
        ) -> Inlet;
        pub fn lsl_destroy_inlet(inlet: Inlet);
        pub fn lsl_open_stream(inlet: Inlet, timeout: f64, ec: *mut i32);
        pub fn lsl_pull_sample_d(
            inlet: Inlet,
            buffer: *mut f64,
            buffer_elements: i32,
            timeout: f64,
            ec: *mut i32,
        ) -> f64;
        pub fn lsl_get_name(info: StreamInfo) -> *const c_char;
    }
}

/// Java default EEG stream name.
pub const DEFAULT_EEG_NAME: &str = "obci_eeg1";
pub const DEFAULT_EEG_TYPE: &str = "EEG";
pub const DEFAULT_MARKER_NAME: &str = "obci_markers";
pub const DEFAULT_MARKER_TYPE: &str = "Markers";

pub fn lsl_linked() -> bool {
    cfg!(has_liblsl)
}

#[allow(dead_code)]
pub struct LslPair {
    #[cfg(has_liblsl)]
    eeg: ffi::Outlet,
    #[cfg(has_liblsl)]
    markers: ffi::Outlet,
    #[allow(dead_code)]
    n_ch: usize,
}

impl LslPair {
    pub fn start(eeg_name: &str, n_ch: usize, sample_rate: f64) -> Result<Self, String> {
        #[cfg(not(has_liblsl))]
        {
            let _ = (eeg_name, n_ch, sample_rate);
            return Err(
                "LSL is not linked in this binary (liblsl.framework was not found at build time)"
                    .into(),
            );
        }
        #[cfg(has_liblsl)]
        {
            if n_ch == 0 {
                return Err("LSL EEG stream needs at least 1 channel".into());
            }
            unsafe {
                let eeg = create_outlet(eeg_name, DEFAULT_EEG_TYPE, n_ch as i32, sample_rate, ffi::CFT_DOUBLE64)?;
                let markers = match create_outlet(
                    DEFAULT_MARKER_NAME,
                    DEFAULT_MARKER_TYPE,
                    1,
                    0.0,
                    ffi::CFT_STRING,
                ) {
                    Ok(m) => m,
                    Err(e) => {
                        ffi::lsl_destroy_outlet(eeg);
                        return Err(e);
                    }
                };
                Ok(Self {
                    eeg,
                    markers,
                    n_ch,
                })
            }
        }
    }

    pub fn push_eeg(&self, sample: &[f64]) -> Result<(), String> {
        #[cfg(not(has_liblsl))]
        {
            let _ = sample;
            return Ok(());
        }
        #[cfg(has_liblsl)]
        {
            if sample.len() < self.n_ch {
                return Err("LSL sample shorter than stream".into());
            }
            let rc = unsafe { ffi::lsl_push_sample_d(self.eeg, sample.as_ptr()) };
            if rc != 0 {
                return Err(format!("lsl_push_sample_d error {rc}"));
            }
            Ok(())
        }
    }

    pub fn push_marker(&self, _timestamp: f64, text: &str) -> Result<(), String> {
        #[cfg(not(has_liblsl))]
        {
            let _ = text;
            return Ok(());
        }
        #[cfg(has_liblsl)]
        {
            let c = CString::new(text).map_err(|e| e.to_string())?;
            let mut ptrs = [c.as_ptr()];
            let rc = unsafe { ffi::lsl_push_sample_str(self.markers, ptrs.as_mut_ptr()) };
            if rc != 0 {
                return Err(format!("lsl_push_sample_str error {rc}"));
            }
            Ok(())
        }
    }
}

impl Drop for LslPair {
    fn drop(&mut self) {
        #[cfg(has_liblsl)]
        unsafe {
            ffi::lsl_destroy_outlet(self.eeg);
            ffi::lsl_destroy_outlet(self.markers);
        }
    }
}

#[cfg(has_liblsl)]
unsafe fn create_outlet(
    name: &str,
    type_: &str,
    n_ch: i32,
    rate: f64,
    fmt: i32,
) -> Result<ffi::Outlet, String> {
    let name = CString::new(name).map_err(|e| e.to_string())?;
    let type_ = CString::new(type_).map_err(|e| e.to_string())?;
    let src = CString::new("OpenBCI_Rust_GUI").unwrap();
    let info = ffi::lsl_create_streaminfo(
        name.as_ptr(),
        type_.as_ptr(),
        n_ch,
        rate,
        fmt,
        src.as_ptr(),
    );
    if info.is_null() {
        return Err("lsl_create_streaminfo returned null".into());
    }
    let out = ffi::lsl_create_outlet(info, 0, 360);
    ffi::lsl_destroy_streaminfo(info);
    if out.is_null() {
        return Err("lsl_create_outlet returned null".into());
    }
    Ok(out)
}

/// Resolve an EEG stream by name (used by the roundtrip test).
#[cfg(all(has_liblsl, test))]
fn resolve_eeg_name(name: &str, timeout_sec: f64) -> Result<String, String> {
    let prop = CString::new("name").unwrap();
    let value = CString::new(name).map_err(|e| e.to_string())?;
    let mut buf: [ffi::StreamInfo; 4] = [ptr::null_mut(); 4];
    let n = unsafe {
        ffi::lsl_resolve_byprop(buf.as_mut_ptr(), 4, prop.as_ptr(), value.as_ptr(), 1, timeout_sec)
    };
    if n <= 0 {
        return Err("no LSL stream found".into());
    }
    let found = unsafe {
        let c = ffi::lsl_get_name(buf[0]);
        if c.is_null() {
            String::new()
        } else {
            std::ffi::CStr::from_ptr(c).to_string_lossy().into_owned()
        }
    };
    unsafe {
        for info in buf.iter().take(n as usize) {
            if !info.is_null() {
                ffi::lsl_destroy_streaminfo(*info);
            }
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linked_flag_matches_build() {
        assert_eq!(lsl_linked(), cfg!(has_liblsl));
    }

    #[test]
    #[cfg(has_liblsl)]
    fn synthetic_outlet_is_visible() {
        let name = format!("obci_eeg_test_{}", std::process::id());
        let pair = LslPair::start(&name, 2, 250.0).expect("create LSL outlet");
        let found = resolve_eeg_name(&name, 2.0).expect("resolve");
        assert_eq!(found, name);

        let prop = CString::new("name").unwrap();
        let value = CString::new(name.as_str()).unwrap();
        let mut buf: [ffi::StreamInfo; 1] = [ptr::null_mut()];
        let n = unsafe {
            ffi::lsl_resolve_byprop(buf.as_mut_ptr(), 1, prop.as_ptr(), value.as_ptr(), 1, 2.0)
        };
        assert!(n > 0 && !buf[0].is_null(), "inlet resolve failed");
        let inlet = unsafe { ffi::lsl_create_inlet(buf[0], 360, 0, 1) };
        unsafe { ffi::lsl_destroy_streaminfo(buf[0]) };
        assert!(!inlet.is_null());
        let mut ec: i32 = 0;
        unsafe { ffi::lsl_open_stream(inlet, 2.0, &mut ec) };
        assert_eq!(ec, 0, "open_stream ec={ec}");
        std::thread::sleep(std::time::Duration::from_millis(200));
        pair.push_eeg(&[1.5, -2.5]).expect("push");
        let mut sample = vec![0.0; 2];
        let mut got = false;
        for _ in 0..20 {
            ec = 0;
            let ts = unsafe {
                ffi::lsl_pull_sample_d(inlet, sample.as_mut_ptr(), 2, 0.25, &mut ec)
            };
            if ec == 0 && ts != 0.0 {
                got = true;
                break;
            }
            pair.push_eeg(&[1.5, -2.5]).ok();
        }
        unsafe { ffi::lsl_destroy_inlet(inlet) };
        assert!(got, "LSL inlet got no sample, last={sample:?} ec={ec}");
        assert!((sample[0] - 1.5).abs() < 1e-3, "got {sample:?}");
    }
}
