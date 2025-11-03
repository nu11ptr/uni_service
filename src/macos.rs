use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use uni_error::{SimpleError, SimpleResult};

use crate::unix::{SERVICE_PERMS, write_file};
use crate::{Result, ServiceManager, ServiceStatus};

const GLOBAL_PATH: &str = "/Library/LaunchDaemons";
const LAUNCH_CTL: &str = "launchctl";

pub(crate) fn make_service_manager(
    name: OsString,
    prefix: OsString,
    user: bool,
) -> SimpleResult<Box<dyn ServiceManager>> {
    LaunchDServiceManager::new(name, prefix, user)
        .map(|mgr| Box::new(mgr) as Box<dyn ServiceManager>)
}

struct LaunchDServiceManager {
    name: OsString,
    prefix: OsString,
    user: bool,
}

impl LaunchDServiceManager {
    fn new(name: OsString, prefix: OsString, user: bool) -> SimpleResult<Self> {
        // launchd? (must be a valid subcommand else returns exit code 1)
        if Self::launch_ctl("list", None).is_ok() {
            Ok(Self { name, prefix, user })
        } else {
            Err(SimpleError::from_context(
                "launchd is not available on this system",
            ))
        }
    }

    pub fn launch_ctl(sub_cmd: &str, target: Option<&OsStr>) -> Result<String> {
        let mut command = Command::new(LAUNCH_CTL);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !sub_cmd.is_empty() {
            command.arg(sub_cmd);
        }

        if let Some(target) = target {
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

    fn path(&self) -> Result<PathBuf> {
        if self.user {
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

    fn make_qualified_name(&self, file_ext: bool) -> OsString {
        let mut s = self.prefix.clone();

        if self.user {
            let uid = unsafe { libc::getuid() };
            s.push("user/");
            s.push(uid.to_string());
            s.push("/");
        } else {
            s.push("system/");
        }

        s.push(&self.name);

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
            self.make_qualified_name(false).to_string_lossy(),
        );

        // Create directories and install
        let path = self.path()?;
        fs::create_dir_all(&path)?;
        let file = path.join(self.make_qualified_name(true));
        write_file(&file, &service, SERVICE_PERMS)?;

        Self::launch_ctl("enable", Some(file.as_ref()))?;
        Ok(())
    }

    fn uninstall(&self) -> Result<()> {
        // First calculate file path and unload
        let path = self.path()?;
        let file = path.join(self.make_qualified_name(true));
        Self::launch_ctl("disable", Some(file.as_ref()))?;

        // ...then wipe service file
        fs::remove_file(file)?;
        Ok(())
    }

    fn start(&self) -> Result<()> {
        Self::launch_ctl("start", Some(self.make_qualified_name(false).as_ref()))?;
        Ok(())
    }

    fn stop(&self) -> Result<()> {
        Self::launch_ctl("stop", Some(self.make_qualified_name(false).as_ref()))?;
        Ok(())
    }

    fn status(&self) -> Result<ServiceStatus> {
        Self::launch_ctl("print", Some(self.make_qualified_name(false).as_ref())).map(|status| {
            if status.contains("state = running") {
                ServiceStatus::Running
            } else {
                ServiceStatus::Stopped
            }
        })
    }
}
