//! Universal service crate for building cross platform OS services

#[cfg(windows)]
mod win_service;

use std::sync::mpsc::channel;

#[cfg(windows)]
use win_service::start_service;

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
