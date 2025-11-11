use std::{
    borrow::Cow,
    ffi::{OsStr, OsString},
    path::PathBuf,
    thread,
    time::{Duration, Instant},
};

use bitflags::bitflags;
use uni_error::{ErrorContext as _, UniKind, UniResult};

// *** make_service_manager ***

#[cfg(target_os = "macos")]
use crate::launchd::make_service_manager;
#[cfg(windows)]
use crate::sc::make_service_manager;
#[cfg(target_os = "linux")]
use crate::systemd::make_service_manager;
#[cfg(not(target_os = "windows"))]
use crate::util;

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
/// Linux/macOS services will only ever be `NotInstalled`, `Running` or `Stopped`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub enum ServiceStatus {
    /// The specified service is not installed.
    NotInstalled,
    /// The specified service is stopped.
    Stopped,
    /// The specified service is starting.
    StartPending,
    /// The specified service is stopping.
    StopPending,
    /// The specified service is running.
    Running,
    /// The specified service is continuing.
    ContinuePending,
    /// The specified service is pausing.
    PausePending,
    /// The specified service is paused.
    Paused,
}

// *** Service Spec ***

/// A specification of a service to be installed.
pub struct ServiceSpec {
    /// The path to the executable to run when the service starts.
    pub path: PathBuf,
    /// The arguments to pass to the executable.
    pub args: Vec<OsString>,
    /// The display name of the service.
    pub display_name: Option<OsString>,
    /// The description of the service.
    pub desc: Option<OsString>,
    /// Whether the service should start automatically when the system boots or user logs in.
    pub autostart: bool,
    /// Whether the service should be restarted if it fails.
    pub restart_on_failure: bool,
    /// User to run the service as.
    pub user: Option<OsString>,
    /// Password to use for the user.
    pub password: Option<OsString>,
    /// Group to run the service as.
    pub group: Option<OsString>,
}

impl ServiceSpec {
    /// Creates a new service specification with the given path to the executable.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            args: vec![],
            display_name: None,
            desc: None,
            autostart: false,
            restart_on_failure: false,
            user: None,
            password: None,
            group: None,
        }
    }

    /// Adds an argument to the executable.
    pub fn arg(mut self, arg: impl Into<OsString>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Sets the display name of the service.
    pub fn display_name(mut self, display_name: impl Into<OsString>) -> Self {
        self.display_name = Some(display_name.into());
        self
    }

    /// Sets the description of the service.
    pub fn desc(mut self, desc: impl Into<OsString>) -> Self {
        self.desc = Some(desc.into().into());
        self
    }

    /// Sets whether the service should start automatically when the system boots or user logs in.
    pub fn set_autostart(mut self) -> Self {
        self.autostart = true;
        self
    }

    /// Sets whether the service should be restarted if it fails.
    pub fn set_restart_on_failure(mut self) -> Self {
        self.restart_on_failure = true;
        self
    }

    /// Sets the user to run the service as.
    pub fn set_user(mut self, user: impl Into<OsString>) -> Self {
        self.user = Some(user.into());
        self
    }

    /// Sets the password to use for the user.
    pub fn set_password(mut self, password: impl Into<OsString>) -> Self {
        self.password = Some(password.into());
        self
    }

    /// Sets the group to run the service as.
    pub fn set_group(mut self, group: impl Into<OsString>) -> Self {
        self.group = Some(group.into());
        self
    }

    pub(crate) fn path_and_args(&self) -> Vec<&OsStr> {
        let mut result = vec![self.path.as_ref()];
        let args = self
            .args
            .iter()
            .map(|arg| <OsString as AsRef<OsStr>>::as_ref(arg));
        result.extend(args);
        result
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn path_and_args_string(&self) -> UniResult<Vec<String>, ServiceErrKind> {
        let combined = self.path_and_args();
        combined
            .iter()
            .map(|arg| util::os_string_to_string(arg))
            .collect()
    }

    #[cfg(target_os = "linux")]
    pub(crate) fn desc_string(&self) -> UniResult<Option<String>, ServiceErrKind> {
        self.desc
            .as_ref()
            .map(|desc| util::os_string_to_string(desc))
            .transpose()
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn user_string(&self) -> UniResult<Option<String>, ServiceErrKind> {
        self.user
            .as_ref()
            .map(|user| util::os_string_to_string(user))
            .transpose()
    }

    #[cfg(not(target_os = "windows"))]
    pub(crate) fn group_string(&self) -> UniResult<Option<String>, ServiceErrKind> {
        self.group
            .as_ref()
            .map(|group| util::os_string_to_string(group))
            .transpose()
    }
}

// *** Service Capabilities ***

bitflags! {
    /// The capabilities of a service manager.
    pub struct ServiceCapabilities: u32 {
        /// The service requires a password when a custom user is used.
        const CUSTOM_USER_REQUIRES_PASSWORD = 1 << 0;
        /// The service supports running as a custom group.
        const SUPPORTS_CUSTOM_GROUP = 1 << 1;
        /// User services require a new logon before they can be started.
        const USER_SERVICES_REQUIRE_NEW_LOGON = 1 << 2;
        /// The service requires autostart to be enabled when restarting on failure is enabled.
        const RESTART_ON_FAILURE_REQUIRES_AUTOSTART = 1 << 3;
        /// The service uses a name prefix.
        const USES_NAME_PREFIX = 1 << 4;
        /// User services require elevated privileges to be installed.
        const ELEVATED_PRIV_REQUIRED_FOR_USER_INSTALL = 1 << 5;
        /// The service supports pending and pause states.
        const SUPPORTS_PENDING_PAUSED_STATES = 1 << 6;
    }
}

// *** Service Manager ***

pub(crate) trait ServiceManager {
    fn install(&self, spec: &ServiceSpec) -> UniResult<(), ServiceErrKind>;

    fn uninstall(&self) -> UniResult<(), ServiceErrKind>;

    fn start(&self) -> UniResult<(), ServiceErrKind>;

    fn stop(&self) -> UniResult<(), ServiceErrKind>;

    fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind>;

    fn capabilities(&self) -> ServiceCapabilities;
}

/// The error type for service management operations.
#[derive(Clone, Debug)]
pub enum ServiceErrKind {
    /// Service management is not available on this platform either because it's not
    /// supported or because the service manager is not detected.
    ServiceManagementNotAvailable,
    /// The service is already installed.
    AlreadyInstalled,
    /// The service is not installed.
    NotInstalled,
    /// The service is in the wrong state for the requested operation.
    WrongState(ServiceStatus),
    /// The status operation timed out. Last status is returned.
    Timeout(ServiceStatus),
    /// The operation timed out. Last error is returned.
    TimeoutError(Box<ServiceErrKind>),
    /// The operation failed because an OS string wasn't valid UTF-8.
    BadUtf8,
    /// The operation failed because a child process exited with a non-zero status.
    BadExitStatus(Option<i32>, String),
    /// The service path was not found.
    ServicePathNotFound,
    /// The operation failed due to insufficient permissions.
    AccessDenied,
    /// The operation failed because a directory was not found.
    DirectoryNotFound,
    /// The operation failed because a user was specified without a password.
    UserRequiresPassword,
    /// The operation failed because restarting on failure is enabled but autostart is not enabled.
    RestartOnFailureRequiresAutostart,
    /// The operation failed because of an I/O error.
    IoError,
    /// The operation failed because of a platform-specific error.
    PlatformError(Option<i64>),

    /// The operation failed because of an unknown error.
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
            ServiceErrKind::BadUtf8 => "Bad UTF-8 encoding".into(),
            ServiceErrKind::BadExitStatus(code, msg) => format!(
                "Bad child process exit status. Code: {:?}. Stderr: {}",
                code, msg
            )
            .into(),
            ServiceErrKind::ServicePathNotFound => "The service path was not found".into(),
            ServiceErrKind::AccessDenied => "Access denied".into(),
            ServiceErrKind::DirectoryNotFound => "Unable to locate the directory".into(),
            ServiceErrKind::UserRequiresPassword => {
                "A user was specified without a password".into()
            }
            ServiceErrKind::RestartOnFailureRequiresAutostart => {
                "Restarting on failure is enabled but autostart is not enabled".into()
            }
            ServiceErrKind::IoError => "An I/O error occurred".into(),
            ServiceErrKind::PlatformError(code) => {
                format!("A platform-specific error occurred. Code: {:?}", code).into()
            }
            ServiceErrKind::Unknown => "Unknown error".into(),
        })
    }
}

// *** UniServiceManager ***

/// A service manager to manage services on the current system. It uses platform-specific implementations
/// behind the scenes to perform the actual service management, but provides a unified interface regardless
/// of the platform.
pub struct UniServiceManager {
    manager: Box<dyn ServiceManager>,
}

impl UniServiceManager {
    /// Creates a new service manager for the given service name. The `prefix` is a java-style
    /// reverse domain name prefix (e.g. `com.example.`) and is only used on macOS (ignored on other
    /// platforms). If `user` is `true`, the service applies directly to the current user only.
    /// On Windows, user level services require administrator privileges to manage and won't start
    /// until the first logon.
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
    pub fn install(&self, spec: &ServiceSpec) -> UniResult<(), ServiceErrKind> {
        match self.status() {
            Ok(ServiceStatus::NotInstalled) => self.manager.install(spec),
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

    /// Gets the capabilities of the service manager.
    pub fn capabilities(&self) -> ServiceCapabilities {
        self.manager.capabilities()
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
