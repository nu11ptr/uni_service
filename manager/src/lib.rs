//! Service management crate which gives a unified interface, but is platform dependent

#[cfg(target_os = "macos")]
mod launchd;
#[cfg(target_os = "linux")]
mod systemd;
#[cfg(not(target_os = "windows"))]
mod unix_util;
#[cfg(windows)]
mod win_service;

use std::ffi::OsString;
use std::path::PathBuf;

#[cfg(target_os = "macos")]
use launchd::make_service_manager;
#[cfg(target_os = "linux")]
use systemd::make_service_manager;
#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
use uni_error::{SimpleError, SimpleResult};
#[cfg(windows)]
use win_service::make_service_manager;

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// *** Status ***

/// The status of a service. Windows services can be in any of these states.
/// Linux/macOS services will only ever either be `Running` or `Stopped`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ServiceStatus {
    Stopped,
    StartPending,
    StopPending,
    Running,
    ContinuePending,
    PausePending,
    Paused,
}

// *** Service Manager ***

/// The service manager is a trait for lifecycle management of a given service
pub trait ServiceManager {
    /// Installs the service. The `program` is the path to the executable to run when the service starts.
    /// The `args` are the arguments to pass to the executable. The `display_name` is the name to display
    /// to the user. The `desc` is the description of the service.
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        display_name: OsString,
        desc: OsString,
    ) -> Result<()>;

    /// Uninstalls the service.
    fn uninstall(&self) -> Result<()>;

    /// Starts the service.
    fn start(&self) -> Result<()>;

    /// Stops the service.
    fn stop(&self) -> Result<()>;

    /// Gets the status of the service.
    fn status(&self) -> Result<ServiceStatus>;
}

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
fn make_service_manager(
    _name: OsString,
    _prefix: OsString,
    _user: bool,
) -> SimpleResult<Box<dyn ServiceManager>> {
    Err(SimpleError::from_context(
        "Service management is not available on this platform",
    ))
}

/// Creates a new service manager for the given service name. The `prefix` is a java-style
/// reverse domain name prefix (e.g. `com.example.`) and is only used on macOS (ignored on other
/// platforms). If `user` is `true`, the service applies directly to the current user only.
/// Windows does not support user-level services, so this is only available on macOS and Linux.
pub fn new_service_manager(
    name: impl Into<OsString>,
    prefix: impl Into<OsString>,
    user: bool,
) -> Result<Box<dyn ServiceManager>> {
    let svc_mgr = make_service_manager(name.into(), prefix.into(), user)?;
    Ok(svc_mgr)
}
