use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};

use uni_error::{SimpleError, SimpleResult};

use crate::unix_util::{SERVICE_PERMS, write_file};
use crate::{Result, ServiceManager, ServiceStatus};

const GLOBAL_PATH: &str = "/etc/systemd/system";
const SYSTEM_CTL: &str = "systemctl";

pub(crate) fn make_service_manager(
    name: OsString,
    _prefix: OsString,
    user: bool,
) -> SimpleResult<Box<dyn ServiceManager>> {
    SystemDServiceManager::new(name, user).map(|mgr| Box::new(mgr) as Box<dyn ServiceManager>)
}

struct SystemDServiceManager {
    name: OsString,
    user: bool,
}

impl SystemDServiceManager {
    fn new(name: OsString, user: bool) -> SimpleResult<Self> {
        let mgr = Self { name, user };

        // systemd exists?
        if mgr.system_ctl(None, true).is_ok() {
            Ok(mgr)
        } else {
            Err(SimpleError::from_context(
                "systemd is not available on this system",
            ))
        }
    }

    fn system_ctl(&self, sub_cmd: Option<&str>, expect_success: bool) -> Result<ExitStatus> {
        let mut command = Command::new(SYSTEM_CTL);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if self.user {
            command.arg("--user");
        }

        if let Some(sub_cmd) = sub_cmd {
            command.arg(sub_cmd).arg(&self.name);
        }

        let output = command.output()?;
        if output.status.success() || !expect_success {
            Ok(output.status)
        } else {
            let msg = String::from_utf8(output.stderr)?;
            Err(SimpleError::from_context(msg.trim().to_string()).into())
        }
    }

    fn path(&self) -> Result<PathBuf> {
        if self.user {
            Ok(dirs::config_dir()
                .ok_or_else(|| {
                    SimpleError::from_context("Unable to locate the user's home directory")
                })?
                .join("systemd")
                .join("user"))
        } else {
            Ok(PathBuf::from(GLOBAL_PATH))
        }
    }
}

impl ServiceManager for SystemDServiceManager {
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        _display_name: OsString,
        desc: OsString,
    ) -> Result<()> {
        // Build service file
        let wanted_by = if self.user {
            "default.target"
        } else {
            "multi-user.target"
        };

        let args = args.join(" ".as_ref());
        let service = format!(
            r#"[Unit]
Description={}

[Service]
ExecStart={} {}
Restart=always

[Install]
WantedBy={wanted_by}
"#,
            desc.to_string_lossy(),
            program.display(),
            args.to_string_lossy(),
        );

        // Create directories and install
        let path = self.path()?;
        fs::create_dir_all(&path)?;
        let file = path.join(format!("{}.service", self.name.to_string_lossy()));
        write_file(&file, &service, SERVICE_PERMS)?;

        self.system_ctl(Some("enable"), true)?;
        Ok(())
    }

    fn uninstall(&self) -> Result<()> {
        // First disable service...
        self.system_ctl(Some("disable"), true)?;

        // ...then wipe service file
        let path = self.path()?;
        let file = path.join(format!("{}.service", self.name.to_string_lossy()));
        fs::remove_file(file)?;
        Ok(())
    }

    fn start(&self) -> Result<()> {
        self.system_ctl(Some("start"), true)?;
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        self.system_ctl(Some("stop"), true)?;
        Ok(())
    }

    fn status(&self) -> Result<ServiceStatus> {
        match self.system_ctl(Some("status"), false) {
            Ok(exit_status) => {
                if exit_status.success() {
                    Ok(ServiceStatus::Running)
                } else {
                    match exit_status.code() {
                        Some(3) => Ok(ServiceStatus::Stopped),
                        // Likely exit code is 4 below, but we'll take anything
                        _ => Err(
                            SimpleError::from_context("Unknown error or service not found").into(),
                        ),
                    }
                }
            }
            Err(err) => Err(err),
        }
    }
}
