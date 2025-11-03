use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use uni_error::SimpleError;

use crate::unix::write_file;
use crate::{Result, ServiceManager, ServiceStatus};

const GLOBAL_PATH: &str = "/Library/LaunchDaemons";
const LAUNCH_CTL: &str = "launchctl";
const SERVICE_PERMS: u32 = 0o644;
const QUALIFIER_PREFIX: &str = "com.apisw.";

pub(crate) fn make_service_manager(name: OsString) -> Option<Box<dyn ServiceManager>> {
    // launchd? (must be a valid subcommand else returns exit code 1)
    if LaunchDServiceManager::launch_ctl("list", "".as_ref()).is_ok() {
        Some(Box::new(LaunchDServiceManager { name }))
    } else {
        None
    }
}

struct LaunchDServiceManager {
    name: OsString,
}

impl LaunchDServiceManager {
    fn launch_ctl(sub_cmd: &str, target: &OsStr) -> Result<String> {
        let mut command = Command::new(LAUNCH_CTL);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !sub_cmd.is_empty() {
            command.arg(sub_cmd);
        }

        if !target.is_empty() {
            command.arg(target);
        }

        let output = command.output()?;
        if output.status.success() {
            Ok(String::from_utf8(output.stdout)?)
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
                .join("Library")
                .join("LaunchAgents"))
        } else {
            Ok(PathBuf::from(GLOBAL_PATH))
        }
    }

    fn make_qualified(name: &OsStr, user: bool, file_ext: bool) -> OsString {
        let mut s = OsString::from(QUALIFIER_PREFIX);
        if user {
            let uid = unsafe { libc::getuid() };
            s.push("user/");
            s.push(uid.to_string());
        } else {
            s.push("system/");
        }
        s.push(name);
        if file_ext {
            s.push(".plist");
        }
        s
    }
}

impl ServiceManager for LaunchDServiceManager {
    fn install(
        &self,
        program: PathBuf,
        args: Vec<OsString>,
        _display_name: OsString,
        _desc: OsString,
        user: bool,
    ) -> Result<()> {
        // Build service file
        let mut new_args: Vec<OsString> = Vec::with_capacity(args.len() + 1);
        new_args.push(program.into());
        new_args.extend(args);
        let args = new_args
            .iter()
            .map(|arg| {
                format!(
                    r#"<string>{}</string>
        "#,
                    arg.to_string_lossy()
                )
            })
            .collect::<Vec<String>>()
            .join("");

        let service = format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
    <dict>
        <key>Label</key>
        <string>{}</string>
        <key>ProgramArguments</key>
        <array>
            {args}</array>
    </dict>
</plist>
"#,
            Self::make_qualified(&self.name, user, false).to_string_lossy(),
        );

        // Create directories and install
        let path = Self::path(user)?;
        fs::create_dir_all(&path)?;
        let file = path.join(Self::make_qualified(&self.name, user, true));
        write_file(&file, &service, SERVICE_PERMS)?;

        Self::launch_ctl("enable", file.as_ref())?;
        Ok(())
    }

    fn uninstall(&self, user: bool) -> Result<()> {
        // First calculate file path and unload
        let path = Self::path(user)?;
        let file = path.join(Self::make_qualified(&self.name, user, true));
        Self::launch_ctl("disable", file.as_ref())?;

        // ...then wipe service file
        fs::remove_file(file)?;
        Ok(())
    }

    fn start(&self, user: bool) -> Result<()> {
        Self::launch_ctl("start", &Self::make_qualified(&self.name, user, false))?;
        Ok(())
    }

    fn stop(&self, user: bool) -> Result<()> {
        Self::launch_ctl("stop", &Self::make_qualified(&self.name, user, false))?;
        Ok(())
    }

    fn status(&self, user: bool) -> Result<ServiceStatus> {
        Self::launch_ctl("print", &Self::make_qualified(&self.name, user, false)).map(|status| {
            if status.contains("state = running") {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            }
        })
    }
}
