#![cfg_attr(docsrs, feature(doc_cfg))]

//! Universal service crate for building cross platform OS services

mod base;
#[doc = include_str!("../README.md")]
mod readme_tests {}
#[cfg(windows)]
mod win_service;

pub use base::BaseService;

use std::{
    sync::mpsc::{Receiver, RecvTimeoutError, channel},
    time::Duration,
};

#[cfg(windows)]
use win_service::start_service;

/// The result type for this crate. The error type is simply a boxed error trait object.
pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

/// A service application.
pub trait ServiceApp {
    /// Returns the name of the service.
    fn name(&self) -> &str;

    /// Called when the service is started. It should do its work as
    /// quickly as possible and return. It should not block indefinitely.
    fn start(&mut self) -> Result<()>;

    /// Called when the service is stopped. It should do any cleanup necessary and return.
    fn stop(self: Box<Self>) -> Result<()>;

    /// Returns whether the service is currently running. If it returns `false`, the service
    /// itself will be stopped.
    fn is_running(&self) -> bool;
}

#[cfg(not(windows))]
fn start_service(app: Box<dyn ServiceApp + Send>) -> Result<()> {
    run_interactive(app)
}

fn run_interactive(mut app: Box<dyn ServiceApp + Send>) -> Result<()> {
    app.start()?;
    wait_for_shutdown(&*app)?;
    app.stop()?;
    Ok(())
}

// NOTE: Windows operates in two possible modes: regular or services mode. UNIX variants operate just in regular mode
/// Executes a service. If being started by the service manager, `service_mode` must be `true`.
/// If being started interactively, `service_mode` must be `false`.
pub fn run_service(app: impl ServiceApp + Send + 'static, service_mode: bool) -> Result<()> {
    let app = Box::new(app);

    if service_mode {
        start_service(app)
    } else {
        run_interactive(app)
    }
}

fn wait_for_shutdown(app: &dyn ServiceApp) -> Result<()> {
    let (tx, rx) = channel();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))?;

    // Wait for termination signal or service to exit
    wait_for_shutdown_or_exit(rx, app)?;
    Ok(())
}

fn wait_for_shutdown_or_exit(shutdown_rx: Receiver<()>, app: &dyn ServiceApp) -> Result<()> {
    while app.is_running() {
        match shutdown_rx.recv_timeout(Duration::from_millis(100)) {
            Ok(_) => break,
            Err(RecvTimeoutError::Timeout) => continue,
            Err(err) => return Err(err.into()),
        }
    }
    Ok(())
}
