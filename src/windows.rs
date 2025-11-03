use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::sync::mpsc::channel;
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::Duration;

use uni_error::SimpleError;
use windows_service::service::{
    ServiceAccess, ServiceControl, ServiceControlAccept, ServiceErrorControl, ServiceExitCode,
    ServiceInfo, ServiceStartType, ServiceState, ServiceStatus, ServiceType,
};
use windows_service::service_control_handler::{ServiceControlHandlerResult, ServiceStatusHandle};
use windows_service::service_manager::ServiceManagerAccess;
use windows_service::{
    define_windows_service, service_control_handler, service_dispatcher, service_manager,
};

use crate::{Result, ServiceApp, ServiceManager};

const MAX_WAIT: u32 = 50; // 5 seconds

static SERVICE_APP: OnceLock<Mutex<Box<dyn ServiceApp + Send>>> = OnceLock::new();

pub(crate) fn start_service(app: Box<dyn ServiceApp + Send>) -> Result<()> {
    let name = app.name().to_string();
    SERVICE_APP.set(Mutex::new(app)).unwrap();
    service_dispatcher::start(&name, ffi_service_main)?;
    Ok(())
}

define_windows_service!(ffi_service_main, service_main);

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
            // TODO: Log error
        }
    }
}

fn service_main(_arguments: Vec<OsString>) -> Result<()> {
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

pub(crate) fn make_service_manager(name: OsString) -> Option<Box<dyn ServiceManager>> {
    Some(Box::new(WinServiceManager { name }))
}

struct WinServiceManager {
    name: OsString,
}

impl WinServiceManager {
    fn check_for_user_service(user: bool) -> Result<()> {
        if user {
            Err(
                SimpleError::from_context("User level services are not available on Windows")
                    .into(),
            )
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
        user: bool,
    ) -> Result<()> {
        Self::check_for_user_service(user)?;

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

    fn uninstall(&self, user: bool) -> Result<()> {
        Self::check_for_user_service(user)?;

        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)?;

        let service_access =
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE;
        let service = service_manager.open_service(&self.name, service_access)?;

        let service_status = service.query_status()?;
        if service_status.current_state != ServiceState::Stopped {
            self.stop(user)?;
        }

        service.delete()?;
        Ok(())
    }

    fn start(&self, user: bool) -> Result<()> {
        Self::check_for_user_service(user)?;

        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)?;

        let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::START;
        let service = service_manager.open_service(&self.name, service_access)?;

        let service_status = service.query_status()?;
        if service_status.current_state != ServiceState::Running {
            if service_status.current_state != ServiceState::StartPending {
                service.start(&[OsStr::new("Starting...")])?;
            }

            let mut started = false;
            let mut count = 0;
            let mut service_status = service.query_status()?;

            while service_status.current_state != ServiceState::Running {
                // Wait for service to stop
                thread::sleep(Duration::from_millis(100));

                service_status = service.query_status()?;

                if service_status.current_state == ServiceState::Running {
                    started = true;
                    break;
                } else {
                    count += 1;
                    if count >= MAX_WAIT {
                        break;
                    }
                }
            }

            if started {
                Ok(())
            } else {
                Err(SimpleError::from_context("Service is not responding and may be hung").into())
            }
        } else {
            Ok(())
        }
    }

    // TODO: Extract a generic-like function and call from both start and stop?
    fn stop(&self, user: bool) -> Result<()> {
        Self::check_for_user_service(user)?;

        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)?;

        let service_access = ServiceAccess::QUERY_STATUS | ServiceAccess::STOP;
        let service = service_manager.open_service(&self.name, service_access)?;

        let service_status = service.query_status()?;
        if service_status.current_state != ServiceState::Stopped {
            if service_status.current_state != ServiceState::StopPending {
                service.stop()?;
            }

            let mut stopped = false;
            let mut count = 0;
            let mut service_status = service.query_status()?;

            while service_status.current_state != ServiceState::Stopped {
                // Wait for service to stop
                thread::sleep(Duration::from_millis(100));

                service_status = service.query_status()?;

                if service_status.current_state == ServiceState::Stopped {
                    stopped = true;
                    break;
                } else {
                    count += 1;
                    if count >= MAX_WAIT {
                        break;
                    }
                }
            }

            if stopped {
                Ok(())
            } else {
                Err(SimpleError::from_context("Service is not responding and may be hung").into())
            }
        } else {
            Ok(())
        }
    }
}
