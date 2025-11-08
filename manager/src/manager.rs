use std::{
    borrow::Cow,
    ffi::OsString,
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use uni_error::{ErrorContext as _, UniKind, UniResult};

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
) -> UniResult<Box<dyn ServiceManager>, ServiceErrKind> {
    Err(ServiceErrKind::ServiceManagementNotAvailable.into_error())
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
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        display_name: OsString,
        desc: OsString,
    ) -> UniResult<(), ServiceErrKind>;

    fn uninstall(&self) -> UniResult<(), ServiceErrKind>;

    fn start(&self) -> UniResult<(), ServiceErrKind>;

    fn stop(&self) -> UniResult<(), ServiceErrKind>;

    fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind>;
}

#[derive(Clone, Debug)]
pub enum ServiceErrKind {
    ServiceManagementNotAvailable,
    AlreadyInstalled,
    NotInstalled,
    WrongState(ServiceStatus),
    Timeout(ServiceStatus),
    TimeoutError(Box<ServiceErrKind>),
    BadUtf8,
    BadExitStatus(Option<i32>, String),
    HomeDirectoryNotFound,
    IoError,

    Unknown,
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
            ServiceErrKind::Unknown => "Unknown error".into(),
            ServiceErrKind::BadUtf8 => "Bad UTF-8 encoding".into(),
            ServiceErrKind::BadExitStatus(code, msg) => format!(
                "Bad child process exit status. Code: {:?}. Stderr: {}",
                code, msg
            )
            .into(),
            ServiceErrKind::HomeDirectoryNotFound => {
                "Unable to locate the user's home directory".into()
            }
            ServiceErrKind::IoError => "An I/O error occurred".into(),
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
        make_service_manager(name.into(), prefix.into(), user).map(|manager| Self { manager })
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
            Ok(ServiceStatus::NotInstalled) => {
                self.manager.install(program, args, display_name, desc)
            }
            Ok(_) => Err(ServiceErrKind::AlreadyInstalled.into_error()),
            Err(e) => Err(e),
        }
    }

    /// Uninstalls the service. After the method returns successfully, the service may or may not be uninstalled yet,
    /// as this is platform-dependent. An error is returned if the service is not installed, if the service
    /// is not stopped, or if the uninstallation fails.
    pub fn uninstall(&self) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::Stopped) => self.manager.uninstall(),
            Ok(status) => Err(ServiceErrKind::WrongState(status).into_error()),
            Err(e) => Err(e),
        }
    }

    /// Starts the service. After the method returns successfully, the service may or may not be started yet,
    /// as this is platform-dependent. An error is returned if the service is not stopped or if the starting
    /// fails.
    pub fn start(&self) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::Stopped) => self.manager.start(),
            Ok(status) => Err(ServiceErrKind::WrongState(status).into_error()),
            Err(e) => Err(e),
        }
    }

    /// Stops the service. After the method returns successfully, the service may or may not be stopped yet,
    /// as this is platform-dependent. An error is returned if the service is not running or if the stopping
    /// fails.
    pub fn stop(&self) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::Running) => self.manager.stop(),
            Ok(status) => Err(ServiceErrKind::WrongState(status).into_error()),
            Err(e) => Err(e),
        }
    }

    /// Gets the current status of the service. It returns an error if the service is not installed
    /// or if the status cannot be determined.
    pub fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind> {
        self.manager.status()
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
                        let kind = err.kind_clone();
                        return Err(err.kind(ServiceErrKind::TimeoutError(Box::new(kind))));
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
}
