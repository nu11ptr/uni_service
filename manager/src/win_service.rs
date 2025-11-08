use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use uni_error::SimpleResult;
use windows_service::service::{
    Service as WindowsService, ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType,
    ServiceState, ServiceType,
};
use windows_service::service_manager;
use windows_service::service_manager::ServiceManagerAccess;

use crate::manager::{Result, ServiceManager, ServiceStatus};

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
        let service = self.open_service(ServiceAccess::DELETE)?;
        tracing::debug!("Deleting service");
        service.delete()?;
        Ok(())
    }

    fn start(&self) -> Result<()> {
        let service = self.open_service(ServiceAccess::START)?;
        tracing::debug!("Starting service");
        service.start(&[OsStr::new("Starting...")])?;
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        let service = self.open_service(ServiceAccess::STOP)?;
        tracing::debug!("Stopping service");
        service.stop()?;
        Ok(())
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
