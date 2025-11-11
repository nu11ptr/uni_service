use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use uni_error::{ResultContext as _, UniError, UniKind as _, UniResult};

use crate::manager::{
    ServiceCapabilities, ServiceErrKind, ServiceManager, ServiceSpec, ServiceStatus,
};
use crate::unix_util::{SERVICE_PERMS, write_file};

const GLOBAL_PATH: &str = "/etc/systemd/system";
const SYSTEM_CTL: &str = "systemctl";

pub(crate) fn make_service_manager(
    name: OsString,
    _prefix: OsString,
    user: bool,
) -> UniResult<Box<dyn ServiceManager>, ServiceErrKind> {
    SystemDServiceManager::new(name, user).map(|mgr| Box::new(mgr) as Box<dyn ServiceManager>)
}

struct SystemDServiceManager {
    name: OsString,
    user: bool,
}

impl SystemDServiceManager {
    fn new(name: OsString, user: bool) -> UniResult<Self, ServiceErrKind> {
        let mgr = Self { name, user };

        // systemd exists?
        if mgr.system_ctl(None).is_ok() {
            Ok(mgr)
        } else {
            Err(UniError::from_kind_context(
                ServiceErrKind::ServiceManagementNotAvailable,
                "systemd is not available on this system",
            ))
        }
    }

    fn system_ctl(&self, sub_cmd: Option<&str>) -> UniResult<(), ServiceErrKind> {
        let mut command = Command::new(SYSTEM_CTL);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if self.user {
            command.arg("--user");
        } else {
            command.arg("--system");
        }

        if let Some(sub_cmd) = sub_cmd {
            command.arg(sub_cmd).arg(&self.name);
        }

        let output = command.output().kind(ServiceErrKind::IoError)?;
        if output.status.success() {
            Ok(())
        } else {
            let msg = String::from_utf8(output.stderr).kind(ServiceErrKind::BadUtf8)?;
            Err(ServiceErrKind::BadExitStatus(output.status.code(), msg).into_error())
        }
    }

    fn path(&self) -> UniResult<PathBuf, ServiceErrKind> {
        if self.user {
            Ok(dirs::config_dir()
                .ok_or_else(|| ServiceErrKind::DirectoryNotFound.into_error())?
                .join("systemd")
                .join("user"))
        } else {
            Ok(PathBuf::from(GLOBAL_PATH))
        }
    }

    fn make_file_name(&self) -> UniResult<PathBuf, ServiceErrKind> {
        let mut filename = OsString::new();
        filename.push(&self.name);
        filename.push(".service");
        Ok(self.path()?.join(filename))
    }
}

impl ServiceManager for SystemDServiceManager {
    fn install(&self, spec: &ServiceSpec) -> UniResult<(), ServiceErrKind> {
        // Build service file
        let wanted_by = if self.user {
            "default.target"
        } else {
            "multi-user.target"
        };

        let args = spec.path_and_args_string()?.join(" ");
        let desc = match spec.desc_string()? {
            Some(desc) => format!("Description={desc}\n"),
            None => String::new(),
        };

        let service = format!(
            r#"[Unit]
{desc}
[Service]
ExecStart={args}
Restart=always

[Install]
WantedBy={wanted_by}
"#
        );

        // Create directories and install
        let path = self.path()?;
        fs::create_dir_all(&path).kind(ServiceErrKind::IoError)?;
        let file = self.make_file_name()?;
        write_file(&file, &service, SERVICE_PERMS)?;

        if spec.autostart {
            self.system_ctl(Some("enable"))?;
        }
        Ok(())
    }

    fn uninstall(&self) -> UniResult<(), ServiceErrKind> {
        // First disable service...
        self.system_ctl(Some("disable"))?;

        // ...then wipe service file
        let file = self.make_file_name()?;
        fs::remove_file(file).kind(ServiceErrKind::IoError)?;
        Ok(())
    }

    fn start(&self) -> UniResult<(), ServiceErrKind> {
        self.system_ctl(Some("start"))
    }

    fn stop(&self) -> UniResult<(), ServiceErrKind> {
        self.system_ctl(Some("stop"))
    }

    fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind> {
        match self.system_ctl(Some("status")) {
            Ok(_) => Ok(ServiceStatus::Running),
            Err(e) if matches!(e.kind_ref(), ServiceErrKind::BadExitStatus(_, _)) => {
                match e.kind_ref() {
                    ServiceErrKind::BadExitStatus(Some(3), _) => Ok(ServiceStatus::Stopped),
                    // Yes, it is a bit weird to turn an error into a successful status, but
                    // this allows us to generalize "wait_for_status" to be able to wait for
                    // uninstallation in addition to other statuses.
                    ServiceErrKind::BadExitStatus(Some(4), _) => Ok(ServiceStatus::NotInstalled),
                    _ => Err(e),
                }
            }
            Err(err) => Err(err),
        }
    }

    fn capabilities(&self) -> ServiceCapabilities {
        ServiceCapabilities::SupportsCustomGroup
    }
}
