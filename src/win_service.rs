use std::ffi::{OsStr, OsString};
use std::sync::mpsc::channel;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use uni_error::SimpleError;
use windows_service::service::{
    ServiceControl, ServiceControlAccept, ServiceExitCode, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::{define_windows_service, service_control_handler, service_dispatcher};

use crate::{Result, ServiceApp, wait_for_shutdown_or_exit};

static SERVICE_APP: OnceLock<Mutex<Option<Box<dyn ServiceApp + Send>>>> = OnceLock::new();

pub(crate) fn start_service(app: Box<dyn ServiceApp + Send>) -> Result<()> {
    let name = app.name().to_string();
    if SERVICE_APP.set(Mutex::new(Some(app))).is_err() {
        return Err(SimpleError::from_context(format!(
            "Only one service can be registered, and '{name}' already is",
        ))
        .into());
    }
    service_dispatcher::start(&name, ffi_service_main)?;
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

// *** Service Control Handler ***

struct ServiceControlHandler(ServiceStatusHandle);

impl ServiceControlHandler {
    fn register<F>(service_name: impl AsRef<OsStr>, event_handler: F) -> Result<Self>
    where
        F: FnMut(ServiceControl) -> ServiceControlHandlerResult + 'static + Send,
    {
        let handle = Self(service_control_handler::register(
            service_name,
            event_handler,
        )?);
        handle.set_status(ServiceState::StartPending)?;
        Ok(handle)
    }

    fn set_status(&self, current_state: ServiceState) -> Result<()> {
        let controls_accepted = if current_state != ServiceState::Stopped {
            ServiceControlAccept::STOP
        } else {
            ServiceControlAccept::empty()
        };

        self.0.set_service_status(ServiceStatus {
            service_type: ServiceType::OWN_PROCESS,
            current_state,
            controls_accepted,
            exit_code: ServiceExitCode::Win32(0),
            checkpoint: 0,
            wait_hint: Duration::default(),
            process_id: None,
        })?;
        Ok(())
    }
}

impl Drop for ServiceControlHandler {
    fn drop(&mut self) {
        if let Err(_err) = self.set_status(ServiceState::Stopped) {
            tracing::error!("Could not set status to Stopped");
        }
    }
}

// *** Service Main ***

fn service_main(_arguments: Vec<OsString>) {
    if let Err(err) = run_service() {
        tracing::error!("An error occurred while running the service: {err}");
    }
}

fn run_service() -> Result<()> {
    tracing::debug!("Service starting...");

    let (shutdown_tx, shutdown_rx) = channel();

    let event_handler_fn = move |event| -> ServiceControlHandlerResult {
        tracing::debug!("Service control event received: {:?}", event);
        match event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => {
                if let Err(_err) = shutdown_tx.send(()) {
                    tracing::error!("Could not send shutdown signal");
                }
                ServiceControlHandlerResult::NoError
            }
            _ => ServiceControlHandlerResult::NotImplemented,
        }
    };

    let mut app = SERVICE_APP
        .get()
        .expect("Missing service app")
        .lock()
        .expect("Mutex poisoned");
    let mut app = app.take().ok_or("Service app not found")?;
    tracing::debug!("Registering service control handler");
    let status_handle = ServiceControlHandler::register(app.name(), event_handler_fn)?;

    tracing::debug!("Calling app's start method");
    app.start()?;
    status_handle.set_status(ServiceState::Running)?;

    tracing::debug!("Waiting for shutdown signal");
    wait_for_shutdown_or_exit(shutdown_rx, &*app)?;

    tracing::debug!("Setting status to StopPending");
    status_handle.set_status(ServiceState::StopPending)?;
    app.stop()?;

    // Drop of handle will automatically set status to Stopped
    tracing::debug!("Service exiting...");
    Ok(())
}
