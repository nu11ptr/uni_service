use std::ffi::{OsStr, OsString};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use uni_error::{SimpleError, SimpleResult};
use windows_service::service::{
    Service as WindowsService, ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType,
    ServiceState, ServiceType,
};
use windows_service::service_manager;
use windows_service::service_manager::ServiceManagerAccess;

use crate::{Result, ServiceManager, ServiceStatus};

const MAX_WAIT: u32 = 50; // 5 seconds

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
        tracing::debug!("Opening service: {:?}", self.name);
        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)?;
        let service = service_manager.open_service(&self.name, flags)?;
        Ok(service)
    }

    fn stop(service: &WindowsService) -> Result<()> {
        tracing::debug!("Attempting to stop service");
        service.stop()?;
        Ok(())
    }

    fn start(service: &WindowsService) -> Result<()> {
        tracing::debug!("Attempting to start service");
        service.start(&[OsStr::new("Starting...")])?;
        Ok(())
    }

    fn change_state(&self, desired_state: ServiceState) -> Result<()> {
        let (service_access, pending_state, change_state_fn): (
            ServiceAccess,
            ServiceState,
            fn(&WindowsService) -> Result<()>,
        ) = match desired_state {
            ServiceState::Stopped => (ServiceAccess::STOP, ServiceState::StopPending, Self::stop),
            ServiceState::Running => (
                ServiceAccess::START,
                ServiceState::StartPending,
                Self::start,
            ),
            _ => {
                unreachable!("Invalid service state");
            }
        };
        tracing::debug!("Opening service: {:?}", self.name);
        let service = self.open_service(ServiceAccess::QUERY_STATUS | service_access)?;

        let service_status = service.query_status()?;
        if service_status.current_state != desired_state {
            tracing::debug!(
                "Service is not in the desired state: {:?}, current state: {:?}",
                desired_state,
                service_status.current_state
            );
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
                    tracing::debug!("Service is now in the desired state: {:?}", desired_state);
                    changed = true;
                    break;
                } else {
                    tracing::debug!(
                        "Service is still not in the desired state: {:?}, current state: {:?}. Trying again...",
                        desired_state,
                        service_status.current_state
                    );
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
            tracing::debug!(
                "Service is already in the desired state: {:?}",
                desired_state
            );
            Ok(())
        }
    }
}

impl ServiceManager for WinServiceManager {
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
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
            launch_arguments: args,
            dependencies: vec![],
            account_name: None, // TODO: Handle alternate users?
            account_password: None,
        };
        tracing::debug!("Creating service: {:?}", service_info);
        let service =
            service_manager.create_service(&service_info, ServiceAccess::CHANGE_CONFIG)?;
        service.set_description(&desc)?;

        Ok(())
    }

    fn uninstall(&self) -> Result<()> {
        tracing::debug!("Opening service: {:?}", self.name);
        let service = self.open_service(
            ServiceAccess::QUERY_STATUS | ServiceAccess::STOP | ServiceAccess::DELETE,
        )?;

        tracing::debug!("Deleting service");
        service.delete()?;
        let service_status = service.query_status()?;
        if service_status.current_state != ServiceState::Stopped {
            self.stop()?;
        }

        // TODO: Consider dropping the service explicitly (either via block or drop) and then
        // query the service to see if it's really gone.
        // (see: https://github.com/mullvad/windows-service-rs/blob/main/examples/uninstall_service.rs)

        Ok(())
    }

    fn start(&self) -> Result<()> {
        self.change_state(ServiceState::Running)
    }

    fn stop(&self) -> Result<()> {
        self.change_state(ServiceState::Stopped)
    }

    fn status(&self) -> Result<ServiceStatus> {
        tracing::debug!("Opening service: {:?}", self.name);
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
