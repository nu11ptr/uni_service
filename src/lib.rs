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
use uni_error::SimpleError;
#[cfg(windows)]
use windows::{make_service_manager, start_service};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A service application.
pub trait ServiceApp: Debug {
    fn name(&self) -> &str;

    fn start(&mut self) -> Result<()>;

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
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        display_name: OsString,
        desc: OsString,
        user: bool,
    ) -> Result<()>;

    fn uninstall(&self, user: bool) -> Result<()>;

    fn start(&self, user: bool) -> Result<()>;

    fn stop(&self, user: bool) -> Result<()>;

    fn status(&self, user: bool) -> Result<ServiceStatus>;
}

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
fn make_service_manager(name: OsString) -> Option<Box<dyn ServiceManager>> {
    None
}

/// Creates a new service manager for the given service name.
pub fn new_service_manager(name: OsString) -> Result<Box<dyn ServiceManager>> {
    match make_service_manager(name) {
        Some(svc_mgr) => Ok(svc_mgr),
        None => Err(SimpleError::from_context(
            "Sorry, service management is not available on this platform",
        )
        .into()),
    }
}
