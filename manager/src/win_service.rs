use std::borrow::Cow;
use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

use uni_error::{ErrorContext as _, ResultContext as _, UniError, UniKind as _, UniResult};

use crate::manager::{
    ServiceCapabilities, ServiceErrKind, ServiceManager, ServiceSpec, ServiceStatus,
};

const SC_EXE: &str = "sc.exe";

pub(crate) fn make_service_manager(
    name: OsString,
    _prefix: OsString,
    user: bool,
) -> UniResult<Box<dyn ServiceManager>, ServiceErrKind> {
    WinServiceManager::new(name, user).map(|mgr| Box::new(mgr) as Box<dyn ServiceManager>)
}

// *** WinServiceManager ***

struct WinServiceManager {
    name: OsString,
    luid: Option<OsString>,
}

impl WinServiceManager {
    fn new(name: OsString, user: bool) -> UniResult<Self, ServiceErrKind> {
        let mut mgr = Self { name, luid: None };

        // If this is a user service, we need to get the LUID of the current user
        let args = if user {
            // Default is only 'active' services which should all have a LUID suffix
            vec!["type=".as_ref(), "userservice".as_ref()]
        } else {
            vec![]
        };

        match mgr.sc("query", None, args) {
            // User service - get the LUID of the current user
            Ok(output) if user => {
                for line in output.lines() {
                    if line.starts_with("SERVICE_NAME:") {
                        // Service Name: <Template>_<LUID>
                        let luid = line.rfind('_').map(|idx| line[idx + 1..].into());
                        if let Some(luid) = luid {
                            mgr.luid = Some(luid);
                            return Ok(mgr);
                        }
                    }
                }

                Err(UniError::from_kind_context(
                    ServiceErrKind::ServiceManagementNotAvailable,
                    "User services are not supported on this system",
                ))
            }
            // System service - just confirm we can query services
            Ok(_) => Ok(mgr),
            Err(e) => Err(e.kind_context(
                ServiceErrKind::ServiceManagementNotAvailable,
                "sc.exe is not available on this system",
            )),
        }
    }

    fn sc(
        &self,
        sub_cmd: &str,
        name: Option<&OsStr>,
        args: Vec<&OsStr>,
    ) -> UniResult<String, ServiceErrKind> {
        let mut command = Command::new(SC_EXE);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        command.arg(sub_cmd);
        if let Some(name) = name {
            command.arg(name);
        }

        for arg in args {
            command.arg(arg);
        }

        tracing::debug!("Executing command: {:?}", command);
        let output = command.output().kind(ServiceErrKind::IoError)?;
        if output.status.success() {
            Ok(String::from_utf8(output.stdout).kind(ServiceErrKind::BadUtf8)?)
        } else {
            let msg = String::from_utf8(output.stderr).kind(ServiceErrKind::BadUtf8)?;
            Err(ServiceErrKind::BadExitStatus(output.status.code(), msg).into_error())
        }
    }

    fn instance_name(&self) -> Cow<'_, OsStr> {
        if let Some(luid) = &self.luid {
            let mut instance_name = OsString::with_capacity(self.name.len() + luid.len() + 1);
            instance_name.push(&self.name);
            instance_name.push("_");
            instance_name.push(luid);
            instance_name.into()
        } else {
            Cow::Borrowed(&self.name)
        }
    }

    fn raw_status(&self, name: &OsStr) -> UniResult<ServiceStatus, ServiceErrKind> {
        match self.sc("query", Some(name), vec![]) {
            Ok(output) => {
                for line in output.lines() {
                    let mut tokens = line.split_whitespace();

                    if tokens.next() == Some("STATE")
                        && tokens.next() == Some(":")
                        // Numeric state code
                        && tokens.next().is_some()
                    {
                        if let Some(state) = tokens.next() {
                            return match state {
                                "RUNNING" => Ok(ServiceStatus::Running),
                                "STOPPED" => Ok(ServiceStatus::Stopped),
                                "START_PENDING" => Ok(ServiceStatus::StartPending),
                                "STOP_PENDING" => Ok(ServiceStatus::StopPending),
                                "CONTINUE_PENDING" => Ok(ServiceStatus::ContinuePending),
                                "PAUSE_PENDING" => Ok(ServiceStatus::PausePending),
                                "PAUSED" => Ok(ServiceStatus::Paused),
                                _ => Err(ServiceErrKind::PlatformError(None).into_error()),
                            };
                        }
                    }
                }

                Err(ServiceErrKind::PlatformError(None).into_error())
            }
            Err(e) => match e.kind_ref() {
                ServiceErrKind::BadExitStatus(Some(2), _) => {
                    Err(e.kind(ServiceErrKind::ServicePathNotFound))
                }
                ServiceErrKind::BadExitStatus(Some(5), _) => {
                    Err(e.kind(ServiceErrKind::AccessDenied))
                }
                // Yes, it is a bit weird to turn an error into a successful status, but
                // this allows us to generalize "wait_for_status" to be able to wait for
                // uninstallation in addition to other statuses.
                ServiceErrKind::BadExitStatus(Some(1060), _) => Ok(ServiceStatus::NotInstalled),
                ServiceErrKind::BadExitStatus(Some(1073), _) => {
                    Err(e.kind(ServiceErrKind::AlreadyInstalled))
                }
                _ => Err(e),
            },
        }
    }
}

impl ServiceManager for WinServiceManager {
    fn install(&self, spec: &ServiceSpec) -> UniResult<(), ServiceErrKind> {
        let type_ = if self.luid.is_none() {
            "own"
        } else {
            "userown"
        };

        let program = spec.path_and_args().join(OsStr::new(" "));

        let start = if spec.autostart { "auto" } else { "demand" };

        let mut create_args: Vec<&OsStr> = vec![
            "type=".as_ref(),
            type_.as_ref(),
            "binPath=".as_ref(),
            &program,
            "start=".as_ref(),
            start.as_ref(),
        ];

        if let Some(display_name) = &spec.display_name {
            create_args.push("DisplayName=".as_ref());
            create_args.push(display_name);
        }

        self.sc("create", Some(&self.name), create_args)?;
        if let Some(desc) = &spec.desc {
            self.sc("description", Some(&self.name), vec![desc])?;
        }

        Ok(())
    }

    fn uninstall(&self) -> UniResult<(), ServiceErrKind> {
        // If we got this far, the status was already checked, but we don't know whether it
        // was the template or the instance that was queried, so we need to check again,
        // and be explicit, as we want to delete the instance if it exists.
        if self.luid.is_some()
            && self.raw_status(&self.instance_name())? != ServiceStatus::NotInstalled
        {
            tracing::debug!("Deleting user service instance");
            self.sc("delete", Some(&self.instance_name()), vec![])?;
        }

        tracing::debug!("Deleting service (or user template)");
        self.sc("delete", Some(&self.name), vec![])?;
        Ok(())
    }

    fn start(&self) -> UniResult<(), ServiceErrKind> {
        tracing::debug!("Starting service");
        self.sc("start", Some(self.instance_name().as_ref()), vec![])?;
        Ok(())
    }

    fn stop(&self) -> UniResult<(), ServiceErrKind> {
        tracing::debug!("Stopping service");
        self.sc("stop", Some(self.instance_name().as_ref()), vec![])?;
        Ok(())
    }

    fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind> {
        let status = self.raw_status(self.instance_name().as_ref())?;

        // User services require a logoff/logon before instances are even created, so
        // we start with the instance attempt above, but if not installed, fallback
        // to the template status
        match (&self.luid, status) {
            (Some(_), ServiceStatus::NotInstalled) => self.raw_status(self.name.as_ref()),
            (_, status) => Ok(status),
        }
    }

    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::UserServicesRequireNewLogon
            | ServiceCapabilities::CustomUserRequiresPassword
    }
}
