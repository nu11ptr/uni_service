use std::{
    borrow::Cow,
    ffi::OsString,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
use uni_error::SimpleResult;
use uni_error::{ResultContext as _, UniError, UniKind, UniResult};

pub(crate) type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;

// *** make_service_manager ***

#[cfg(target_os = "macos")]
use crate::launchd::make_service_manager;
#[cfg(target_os = "linux")]
use crate::systemd::make_service_manager;
#[cfg(windows)]
use crate::win_service::make_service_manager;

#[cfg(all(
    not(target_os = "windows"),
    not(target_os = "linux"),
    not(target_os = "macos")
))]
fn make_service_manager(
    _name: OsString,
    _prefix: OsString,
    _user: bool,
) -> SimpleResult<Box<dyn ServiceManager>> {
    Err(SimpleError::from_context(
        "Service management is not available on this platform",
    ))
}

// *** Status ***

/// The status of a service. Windows services can be in any of these states.
/// Linux/macOS services will only ever either be `Running` or `Stopped`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ServiceStatus {
    NotInstalled,
    Stopped,
    StartPending,
    StopPending,
    Running,
    ContinuePending,
    PausePending,
    Paused,
}

// *** Service Manager ***

/// The service manager is a trait for lifecycle management of a given service
pub(crate) trait ServiceManager {
    /// Installs the service. The `program` is the path to the executable to run when the service starts.
    /// The `args` are the arguments to pass to the executable. The `display_name` is the name to display
    /// to the user. The `desc` is the description of the service.
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        display_name: OsString,
        desc: OsString,
    ) -> Result<()>;

    /// Uninstalls the service.
    fn uninstall(&self) -> Result<()>;

    /// Starts the service.
    fn start(&self) -> Result<()>;

    /// Stops the service.
    fn stop(&self) -> Result<()>;

    /// Gets the status of the service.
    fn status(&self) -> Result<ServiceStatus>;
}

#[derive(Clone, Debug)]
pub enum ServiceErrKind {
    ServiceManagementNotAvailable,
    AlreadyInstalled,
    NotInstalled,
    WrongState(ServiceStatus),
    Timeout(ServiceStatus),
    TimeoutError(Box<ServiceErrKind>),
    UnknownError,
}

impl UniKind for ServiceErrKind {
    fn context(&self) -> Option<Cow<'static, str>> {
        Some(match self {
            ServiceErrKind::ServiceManagementNotAvailable => {
                "Service management is not available on this platform".into()
            }
            ServiceErrKind::AlreadyInstalled => "Service is already installed".into(),
            ServiceErrKind::NotInstalled => "Service is not installed".into(),
            ServiceErrKind::WrongState(status) => format!(
                "Service is in the wrong state for the requested operation. Current status: {:?}",
                status
            )
            .into(),
            ServiceErrKind::Timeout(status) => format!(
                "Timeout waiting for service status. Last status: {:?}",
                status
            )
            .into(),
            ServiceErrKind::TimeoutError(kind) => {
                format!("Timeout waiting for service status. Last error: {:?}", kind).into()
            }
            ServiceErrKind::UnknownError => "Unknown error".into(),
        })
    }
}

// *** UniServiceManager ***

pub struct UniServiceManager {
    manager: Box<dyn ServiceManager>,
}

impl UniServiceManager {
    /// Creates a new service manager for the given service name. The `prefix` is a java-style
    /// reverse domain name prefix (e.g. `com.example.`) and is only used on macOS (ignored on other
    /// platforms). If `user` is `true`, the service applies directly to the current user only.
    /// Windows does not support user-level services, so this is only available on macOS and Linux.
    pub fn new(
        name: impl Into<OsString>,
        prefix: impl Into<OsString>,
        user: bool,
    ) -> UniResult<Self, ServiceErrKind> {
        let manager = make_service_manager(name.into(), prefix.into(), user)
            .kind(ServiceErrKind::UnknownError)?;
        Ok(Self { manager })
    }

    /// Installs the service. The `program` is the path to the executable to run when the service starts.
    /// The `args` are the arguments to pass to the executable. The `display_name` is the name to display
    /// to the user. The `desc` is the description of the service. After the method returns successfully, the
    /// service may or may not be installed yet, as this is platform-dependent. An error is returned if the
    /// service is already installed or if the installation fails.
    pub fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        display_name: OsString,
        desc: OsString,
    ) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(_) => Err(ServiceErrKind::AlreadyInstalled.into_error()),
            Err(_) => self
                .manager
                .install(program, args, display_name, desc)
                .map_err(|e| UniError::from_kind_boxed(ServiceErrKind::UnknownError, e)),
        }
    }

    /// Uninstalls the service. After the method returns successfully, the service may or may not be uninstalled yet,
    /// as this is platform-dependent. An error is returned if the service is not installed, if the service
    /// is not stopped, or if the uninstallation fails.
    pub fn uninstall(&self) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::Stopped) => self
                .manager
                .uninstall()
                .map_err(|e| UniError::from_kind_boxed(ServiceErrKind::UnknownError, e)),
            Ok(status) => Err(ServiceErrKind::WrongState(status).into_error()),
            Err(_) => Err(ServiceErrKind::NotInstalled.into_error()),
        }
    }

    /// Starts the service. After the method returns successfully, the service may or may not be started yet,
    /// as this is platform-dependent. An error is returned if the service is not stopped or if the starting
    /// fails.
    pub fn start(&self) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::Stopped) => self
                .manager
                .start()
                .map_err(|e| UniError::from_kind_boxed(ServiceErrKind::UnknownError, e)),
            Ok(status) => Err(ServiceErrKind::WrongState(status).into_error()),
            Err(_) => Err(ServiceErrKind::NotInstalled.into_error()),
        }
    }

    /// Stops the service. After the method returns successfully, the service may or may not be stopped yet,
    /// as this is platform-dependent. An error is returned if the service is not running or if the stopping
    /// fails.
    pub fn stop(&self) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::Running) => self
                .manager
                .stop()
                .map_err(|e| UniError::from_kind_boxed(ServiceErrKind::UnknownError, e)),
            Ok(status) => Err(ServiceErrKind::WrongState(status).into_error()),
            Err(_) => Err(ServiceErrKind::NotInstalled.into_error()),
        }
    }

    /// Gets the current status of the service. It returns an error if the service is not installed
    /// or if the status cannot be determined.
    pub fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind> {
        self.manager
            .status()
            .map_err(|e| UniError::from_kind_boxed(ServiceErrKind::UnknownError, e))
    }

    /// Waits for the service to reach the desired status. It returns an error if the service is not installed
    /// the status cannot be determined, or if the service does not reach the desired status before the timeout.
    pub fn wait_for_status(
        &self,
        desired_status: ServiceStatus,
        timeout: Duration,
    ) -> UniResult<(), ServiceErrKind> {
        let start_time = Instant::now();

        loop {
            let (last_status, last_error) = match self.status() {
                Ok(s) => {
                    if s == desired_status {
                        return Ok(());
                    }

                    (Some(s), None)
                }
                Err(e) => (None, Some(e)),
            };

            if start_time.elapsed() > timeout {
                match (last_status, last_error) {
                    (None, Some(err)) => {
                        return Err(
                            ServiceErrKind::TimeoutError(Box::new(err.kind_clone())).into_error()
                        );
                    }
                    (Some(s), None) => {
                        return Err(ServiceErrKind::Timeout(s).into_error());
                    }
                    _ => unreachable!(),
                }
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }

    /// Temporary method until UniError<ServiceErrorKind> is used by boxed service manager.
    /// Afte that, they can return ServiceErrKind::NotInstalled instead of an error.
    pub fn wait_for_status_error(
        &self,
        timeout: Duration,
    ) -> UniResult<UniError<ServiceErrKind>, ServiceErrKind> {
        let start_time = Instant::now();

        loop {
            let last_status = match self.status() {
                Ok(s) => s,
                Err(e) => return Ok(e),
            };

            if start_time.elapsed() > timeout {
                return Err(ServiceErrKind::Timeout(last_status).into_error());
            } else {
                thread::sleep(Duration::from_millis(50));
            }
        }
    }
}
