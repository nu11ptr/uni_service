use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use uni_error::SimpleError;

use crate::unix::write_file;
use crate::{Result, ServiceManager};

const GLOBAL_PATH: &str = "/etc/systemd/system";
const SYSTEM_CTL: &str = "systemctl";
const SERVICE_PERMS: u32 = 0o644;

pub(crate) fn make_service_manager(name: OsString) -> Option<Box<dyn ServiceManager>> {
    // systemd?
    if SystemDServiceManager::system_ctl("", &name, false).is_ok() {
        Some(Box::new(SystemDServiceManager { name }))
    } else {
        None
    }
}

struct SystemDServiceManager {
    name: OsString,
}

impl SystemDServiceManager {
    fn system_ctl(sub_cmd: &str, name: &OsStr, user: bool) -> Result<()> {
        let mut command = Command::new(SYSTEM_CTL);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if user {
            command.arg("--user");
        }

        if !sub_cmd.is_empty() {
            command.arg(sub_cmd).arg(name);
        }

        let output = command.output()?;
        if output.status.success() {
            Ok(())
        } else {
            let msg = String::from_utf8(output.stderr)?;
            Err(SimpleError::from_context(msg.trim().to_string()).into())
        }
    }

    fn path(user: bool) -> Result<PathBuf> {
        if user {
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
        user: bool,
    ) -> Result<()> {
        // Build service file
        let wanted_by = if user {
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
        let path = Self::path(user)?;
        fs::create_dir_all(&path)?;
        let file = path.join(format!("{}.service", self.name.to_string_lossy()));
        write_file(&file, &service, SERVICE_PERMS)?;

        Self::system_ctl("enable", &self.name, user)
    }

    fn uninstall(&self, user: bool) -> Result<()> {
        // First disable service...
        Self::system_ctl("disable", &self.name, user)?;

        // ...then wipe service file
        let path = Self::path(user)?;
        let file = path.join(format!("{}.service", self.name.to_string_lossy()));
        fs::remove_file(file)?;
        Ok(())
    }

    fn start(&self, user: bool) -> Result<()> {
        Self::system_ctl("start", &self.name, user)
    }

    fn stop(&self, user: bool) -> Result<()> {
        Self::system_ctl("stop", &self.name, user)
    }
}
