use std::env;
use std::sync::atomic::{AtomicBool, Ordering};

static DEBUG_ENABLED: AtomicBool = AtomicBool::new(false);
static DEBUG_VERBOSE_ENABLED: AtomicBool = AtomicBool::new(false);

pub fn set_debug_logging(enabled: bool) {
    DEBUG_ENABLED.store(enabled, Ordering::Relaxed);
}

#[allow(dead_code)]
pub fn set_verbose_debug_logging(enabled: bool) {
    DEBUG_VERBOSE_ENABLED.store(enabled, Ordering::Relaxed);
}

pub fn progress_log(message: &str) {
    eprintln!("[gb-camera] {message}");
}

pub fn debug_log(message: &str) {
    if DEBUG_ENABLED.load(Ordering::Relaxed) || env::var_os("GB_CAMERA_DEBUG").is_some() {
        eprintln!("[gb-camera-debug] {message}");
    }
}

pub fn debug_log_verbose(message: &str) {
    if DEBUG_VERBOSE_ENABLED.load(Ordering::Relaxed)
        || env::var_os("GB_CAMERA_DEBUG_VERBOSE").is_some()
    {
        eprintln!("[gb-camera-debug] {message}");
    }
}
