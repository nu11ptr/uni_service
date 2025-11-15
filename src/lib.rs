#![cfg_attr(docsrs, feature(doc_cfg))]

//! Universal service crate for building cross platform OS services

mod base;
#[doc = include_str!("../README.md")]
mod readme_tests {}
#[cfg(windows)]
mod win_service;

pub use base::BaseService;

use std::sync::mpsc::channel;

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
    fn stop(&mut self) -> Result<()>;
}

#[cfg(not(windows))]
fn start_service(mut app: Box<dyn ServiceApp + Send>) -> Result<()> {
    run_interactive(&mut *app)
}

fn run_interactive(app: &mut (dyn ServiceApp + Send + 'static)) -> Result<()> {
    app.start()?;
    wait_for_shutdown()?;
    app.stop()?;
    Ok(())
}

// NOTE: Windows operates in two possible modes: regular or services mode. UNIX variants operate just in regular mode
/// Executes a service. If being started by the service manager, `service_mode` must be `true`.
/// If being started interactively, `service_mode` must be `false`.
pub fn run_service(mut app: impl ServiceApp + Send + 'static, service_mode: bool) -> Result<()> {
    if service_mode {
        start_service(Box::new(app))
    } else {
        run_interactive(&mut app)
    }
}

fn wait_for_shutdown() -> Result<()> {
    let (tx, rx) = channel();
    ctrlc::set_handler(move || tx.send(()).expect("Could not send signal on channel."))?;

    // Wait for termination signal
    rx.recv()?;
    Ok(())
}
