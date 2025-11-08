use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use uni_error::{ErrorContext as _, ResultContext as _, UniResult};
use windows_service::service::{
    Service as WindowsService, ServiceAccess, ServiceErrorControl, ServiceInfo, ServiceStartType,
    ServiceState, ServiceType,
};
use windows_service::service_manager::ServiceManagerAccess;
use windows_service::{Error, service_manager};

use crate::manager::{ServiceErrKind, ServiceManager, ServiceStatus};

pub(crate) fn make_service_manager(
    name: OsString,
    _prefix: OsString,
    _user: bool,
) -> UniResult<Box<dyn ServiceManager>, ServiceErrKind> {
    Ok(Box::new(WinServiceManager { name }))
}

// *** WinServiceManager ***

struct WinServiceManager {
    name: OsString,
}

impl WinServiceManager {
    fn open_service(&self, flags: ServiceAccess) -> UniResult<WindowsService, ServiceErrKind> {
        tracing::debug!("Opening service: {:?}", self.name);
        let manager_access = ServiceManagerAccess::CONNECT;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)
                .kind(ServiceErrKind::Unknown)?;
        match service_manager.open_service(&self.name, flags) {
            Ok(service) => Ok(service),
            Err(Error::Winapi(err)) if err.raw_os_error() == Some(1060) => {
                // Put the error back the way it was
                let err = Error::Winapi(err);
                Err(err.kind(ServiceErrKind::NotInstalled))
            }
            Err(e) => Err(e.kind(ServiceErrKind::Unknown)),
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
    ) -> UniResult<(), ServiceErrKind> {
        let manager_access = ServiceManagerAccess::CONNECT | ServiceManagerAccess::CREATE_SERVICE;
        let service_manager =
            service_manager::ServiceManager::local_computer(None::<&str>, manager_access)
                .kind(ServiceErrKind::Unknown)?;

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
        let service = service_manager
            .create_service(&service_info, ServiceAccess::CHANGE_CONFIG)
            .kind(ServiceErrKind::Unknown)?;
        service
            .set_description(&desc)
            .kind(ServiceErrKind::Unknown)?;

        Ok(())
    }

    fn uninstall(&self) -> UniResult<(), ServiceErrKind> {
        let service = self.open_service(ServiceAccess::DELETE)?;
        tracing::debug!("Deleting service");
        service.delete().kind(ServiceErrKind::Unknown)?;
        Ok(())
    }

    fn start(&self) -> UniResult<(), ServiceErrKind> {
        let service = self.open_service(ServiceAccess::START)?;
        tracing::debug!("Starting service");
        service
            .start(&[OsStr::new("Starting...")])
            .kind(ServiceErrKind::Unknown)?;
        Ok(())
    }

    fn stop(&self) -> UniResult<(), ServiceErrKind> {
        let service = self.open_service(ServiceAccess::STOP)?;
        tracing::debug!("Stopping service");
        service.stop().kind(ServiceErrKind::Unknown)?;
        Ok(())
    }

    fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind> {
        let service = match self.open_service(ServiceAccess::QUERY_STATUS) {
            Ok(service) => service,
            Err(e) if matches!(e.kind_ref(), ServiceErrKind::NotInstalled) => {
                return Ok(ServiceStatus::NotInstalled);
            }
            Err(e) => return Err(e),
        };
        let service_status = service.query_status().kind(ServiceErrKind::Unknown)?;

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
