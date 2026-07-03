use std::ffi::{CStr, CString};
use std::os::raw::{c_char, c_int, c_void};
use std::path::Path;
use std::ptr;

unsafe extern "C" {
    fn metafy_macos_segmented_recorder_create(
        manifest_path: *const c_char,
        width: i64,
        height: i64,
        frame_rate: i64,
        max_frames_per_chunk: i64,
        recorder_out: *mut *mut c_void,
        error_out: *mut *mut c_char,
    ) -> c_int;
    fn metafy_macos_segmented_recorder_append_frame(
        recorder: *mut c_void,
        bgra_bytes: *const u8,
        byte_count: usize,
        elapsed_ms: i64,
        display_time_ms: i64,
        error_out: *mut *mut c_char,
    ) -> c_int;
    fn metafy_macos_segmented_recorder_finish(
        recorder: *mut c_void,
        error_out: *mut *mut c_char,
    ) -> c_int;
    fn metafy_macos_segmented_recorder_destroy(recorder: *mut c_void);
    fn metafy_macos_free_string(value: *mut c_char);
}

pub struct MacosSegmentedVideoWriter {
    recorder: *mut c_void,
    finished: bool,
}

impl MacosSegmentedVideoWriter {
    pub fn create(
        manifest_path: &Path,
        width: i64,
        height: i64,
        frame_rate: i64,
        max_frames_per_chunk: i64,
    ) -> Result<Self, String> {
        let manifest_path = path_c_string(manifest_path)?;
        let mut recorder: *mut c_void = ptr::null_mut();
        let mut native_error: *mut c_char = ptr::null_mut();
        let status = unsafe {
            metafy_macos_segmented_recorder_create(
                manifest_path.as_ptr(),
                width,
                height,
                frame_rate,
                max_frames_per_chunk,
                &mut recorder,
                &mut native_error,
            )
        };
        if status != 0 || recorder.is_null() {
            return Err(format!(
                "Native macOS chunked recorder setup failed: {}",
                take_native_error(native_error)
            ));
        }

        Ok(Self {
            recorder,
            finished: false,
        })
    }

    pub fn append_frame(
        &mut self,
        bgra_bytes: &[u8],
        elapsed_ms: i64,
        display_time_ms: i64,
    ) -> Result<(), String> {
        let mut native_error: *mut c_char = ptr::null_mut();
        let status = unsafe {
            metafy_macos_segmented_recorder_append_frame(
                self.recorder,
                bgra_bytes.as_ptr(),
                bgra_bytes.len(),
                elapsed_ms,
                display_time_ms,
                &mut native_error,
            )
        };
        if status != 0 {
            return Err(format!(
                "Native macOS chunked recorder append failed: {}",
                take_native_error(native_error)
            ));
        }

        Ok(())
    }

    pub fn finish(&mut self) -> Result<(), String> {
        if self.finished {
            return Ok(());
        }

        let mut native_error: *mut c_char = ptr::null_mut();
        let status =
            unsafe { metafy_macos_segmented_recorder_finish(self.recorder, &mut native_error) };
        if status != 0 {
            return Err(format!(
                "Native macOS chunked recorder finish failed: {}",
                take_native_error(native_error)
            ));
        }
        self.finished = true;

        Ok(())
    }
}

impl Drop for MacosSegmentedVideoWriter {
    fn drop(&mut self) {
        if !self.recorder.is_null() {
            unsafe {
                metafy_macos_segmented_recorder_destroy(self.recorder);
            }
            self.recorder = ptr::null_mut();
        }
    }
}

fn path_c_string(path: &Path) -> Result<CString, String> {
    CString::new(path_to_string(path))
        .map_err(|_| format!("Path contains an unsupported NUL byte: {}", path.display()))
}

fn path_to_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn take_native_error(error: *mut c_char) -> String {
    if error.is_null() {
        return "native encoder returned no error detail".to_owned();
    }

    let message = unsafe { CStr::from_ptr(error).to_string_lossy().into_owned() };
    unsafe {
        metafy_macos_free_string(error);
    }
    message
}
