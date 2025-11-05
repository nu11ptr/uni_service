use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use uni_error::{SimpleError, SimpleResult};
use windows_service::service::{
    Service as WindowsService, ServiceAccess, ServiceControl, ServiceControlAccept,
    ServiceErrorControl, ServiceExitCode, ServiceInfo, ServiceStartType, ServiceState,
    ServiceStatus as WindowsServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::service_manager::ServiceManagerAccess;
use windows_service::{
    define_windows_service, service_control_handler, service_dispatcher, service_manager,
};

use crate::{Result, ServiceApp, ServiceManager, ServiceStatus};

const MAX_WAIT: u32 = 50; // 5 seconds

static SERVICE_APP: OnceLock<Mutex<Box<dyn ServiceApp + Send>>> = OnceLock::new();

pub(crate) fn start_service(app: Box<dyn ServiceApp + Send>) -> Result<()> {
    let name = app.name().to_string();
    if let Err(_) = SERVICE_APP.set(Mutex::new(app)) {
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

        self.0.set_service_status(WindowsServiceStatus {
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
            // TODO: Log error
        }
    }
}

// *** Service Main ***

fn service_main(_arguments: Vec<OsString>) {
    if let Err(_err) = run_service() {
        // TODO: Log error
    }
}

fn run_service() -> Result<()> {
    let (shutdown_tx, shutdown_rx) = channel();

    let event_handler_fn = move |event| -> ServiceControlHandlerResult {
        match event {
            ServiceControl::Interrogate => ServiceControlHandlerResult::NoError,
            ServiceControl::Stop => {
                if let Err(_err) = shutdown_tx.send(()) {
                    // TODO: Log error
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
    let status_handle = ServiceControlHandler::register(app.name(), event_handler_fn)?;

    app.start()?;
    status_handle.set_status(ServiceState::Running)?;

    shutdown_rx.recv()?;

    status_handle.set_status(ServiceState::StopPending)?;
    app.stop()

    // Drop of handle will automatically set status to Stopped
}

// *** make_service_manager ***

pub(crate) fn make_service_manager(
    name: OsString,
    _prefix: OsString,
    _user: bool,
) -> SimpleResult<Box<dyn ServiceManager>> {
    Ok(Box::new(WinServiceManager { name }))
}

// *** WinServiceManager ***

struct WinServiceManager {
    name: OsString,
}

impl WinServiceManager {
    fn open_service(&self, flags: ServiceAccess) -> Result<WindowsService> {
        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)?;
        let service = service_manager.open_service(&self.name, flags)?;
        Ok(service)
    }

    fn stop(service: &WindowsService) -> Result<()> {
        service.stop()?;
        Ok(())
    }

    fn start(service: &WindowsService) -> Result<()> {
        service.start(&[OsStr::new("Starting...")])?;
        Ok(())
    }

    fn change_state(&self, desired_state: ServiceState) -> Result<()> {
        let (service_access, pending_state, change_state_fn): (
            ServiceAccess,
            ServiceState,
            fn(&WindowsService) -> Result<()>,
        ) = match desired_state {
            ServiceState::Stopped => (ServiceAccess::STOP, ServiceState::Stopped, Self::stop),
            ServiceState::Running => (
                ServiceAccess::START,
                ServiceState::StartPending,
                Self::start,
            ),
            _ => {
                unreachable!("Invalid service state");
            }
        };
        let service = self.open_service(ServiceAccess::QUERY_STATUS | service_access)?;

        let service_status = service.query_status()?;
        if service_status.current_state != desired_state {
            if service_status.current_state != pending_state {
                change_state_fn(&service)?;
            }

            let mut changed = false;
            let mut count = 0;
            let mut service_status = service.query_status()?;

            while service_status.current_state != desired_state {
                // Wait for service to change state
                thread::sleep(Duration::from_millis(100));

                service_status = service.query_status()?;

                if service_status.current_state == desired_state {
                    changed = true;
                    break;
                } else {
                    count += 1;
                    if count >= MAX_WAIT {
                        break;
                    }
                }
            }

            if changed {
                Ok(())
            } else {
                Err(SimpleError::from_context("Service is not responding and may be hung").into())
            }
        } else {
            Ok(())
        }
    }
}

impl ServiceManager for WinServiceManager {
    fn install(
        &self,
        program: PathBuf,
        _args: Vec<OsString>,
        display_name: OsString,
        desc: OsString,
    ) -> Result<()> {
        let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)?;

        let service_info = ServiceInfo {
            name: self.name.clone(),
            display_name,
            service_type: ServiceType::OWN_PROCESS,
            start_type: ServiceStartType::OnDemand,
            error_control: ServiceErrorControl::Normal,
            executable_path: program,
            launch_arguments: vec![OsString::from("service")],
            dependencies: vec![],
            account_name: None, // TODO: Handle alternate users?
            account_password: None,
        };
        let service =
            service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
        service.set_description(&desc)?;

        Ok(())
    }

    fn uninstall(&self) -> Result<()> {
        let service = self.open_service(
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        )?;

        let service_status = service.query_status()?;
        if service_status.current_state != ServiceState::Stopped {
            self.stop()?;
        }

        service.delete()?;
        Ok(())
    }

    fn start(&self) -> Result<()> {
        self.change_state(ServiceState::Running)
    }

    fn stop(&self) -> Result<()> {
        self.change_state(ServiceState::Stopped)
    }

    fn status(&self) -> Result<ServiceStatus> {
        let service = self.open_service(ServiceAccess::QUERY_STATUS)?;
        let service_status = service.query_status()?;

        match service_status.current_state {
            ServiceState::Running => Ok(ServiceStatus::Running),
            ServiceState::Stopped => Ok(ServiceStatus::Stopped),
            ServiceState::StartPending => Ok(ServiceStatus::StartPending),
            ServiceState::StopPending => Ok(ServiceStatus::StopPending),
            ServiceState::ContinuePending => Ok(ServiceStatus::ContinuePending),
            ServiceState::PausePending => Ok(ServiceStatus::PausePending),
            ServiceState::Paused => Ok(ServiceStatus::Paused),
        }
    }
}
