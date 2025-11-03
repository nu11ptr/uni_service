//! Service management crate which gives a unified interface, but is platform dependent

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

use std::fmt::Debug;
use std::path::PathBuf;
use std::{ffi::OsString, sync::mpsc::channel};

#[cfg(target_os = "linux")]
use linux::make_service_manager;
#[cfg(target_os = "macos")]
use macos::make_service_manager;
#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
use uni_error::{SimpleError, SimpleResult};
#[cfg(windows)]
use windows::{make_service_manager, start_service};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A service application.
pub trait ServiceApp: Debug {
    /// Returns the name of the service.
    fn name(&self) -> &str;

    /// Called when the service is started. It should do its work as
    /// quickly as possible and return. It should not block indefinitely.
    fn start(&mut self) -> Result<()>;

    /// Called when the service is stopped. It should do any cleanup necessary and return.
    fn stop(&mut self) -> Result<()>;
}

#[cfg(not(windows))]
fn start_service(app: Box<dyn ServiceApp + Send>) -> Result<()> {
    // Won't endlessly loop because this is only called when service_mode is true
    run_service(app, false)
}

// NOTE: Windows operates in two possible modes: regular or services mode. UNIX variants operate just in regular mode
/// Executes a service. If being started by the service manager, `service_mode` must be `true`.
/// If being started interactively, `service_mode` must be `false`.
pub fn run_service(mut app: Box<dyn ServiceApp + Send>, service_mode: bool) -> Result<()> {
    if service_mode {
        start_service(app)
    } else {
        app.start()?;
        wait_for_shutdown()?;
        app.stop()?;
        Ok(())
    }
}

fn wait_for_shutdown() -> Result<()> {
    let (tx, rx) = channel();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))?;

    // Wait for termination signal
    rx.recv()?;
    Ok(())
}

// *** Status ***

/// The status of a service. Windows services can be in any of these states.
/// Linux/macOS services will only ever either be `Running` or `Stopped`.
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
