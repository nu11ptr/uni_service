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
#[cfg(unix)]
use unix::start_service;
#[cfg(windows)]
use windows::{make_service_manager, start_service};

pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

pub trait ServiceApp: Debug {
    fn name(&self) -> &str;

    fn start(&mut self) -> Result<()>;

    fn stop(&mut self) -> Result<()>;
}

// Windows operates in two possible modes: regular or services mode
// UNIX variants operate just in regular mode
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
}

pub fn new_service_manager(name: OsString) -> Result<Box<dyn ServiceManager>> {
    match make_service_manager(name) {
        Some(svc_mgr) => Ok(svc_mgr),
        None => Err(SimpleError::from_context(
            "Sorry, service management is not available on this platform",
        )
        .into()),
    }
}
