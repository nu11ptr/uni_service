//! Service management crate which gives a unified interface, but is platform dependent

#[cfg(target_os = "macos")]
mod launchd;
mod manager;
#[cfg(target_os = "linux")]
mod systemd;
#[cfg(not(target_os = "windows"))]
mod unix_util;
mod util;
#[cfg(windows)]
mod win_service;

pub use manager::*;
