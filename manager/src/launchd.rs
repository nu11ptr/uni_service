use std::ffi::{OsStr, OsString};
use std::fs;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use uni_error::{ResultContext as _, UniError, UniKind as _, UniResult};

use crate::ServiceErrKind;
use crate::manager::{ServiceManager, ServiceStatus};
use crate::unix_util::{SERVICE_PERMS, write_file};

const GLOBAL_PATH: &str = "/Library/LaunchDaemons";
const LAUNCH_CTL: &str = "launchctl";

pub(crate) fn make_service_manager(
    name: OsString,
    prefix: OsString,
    user: bool,
) -> UniResult<Box<dyn ServiceManager>, ServiceErrKind> {
    LaunchDServiceManager::new(name, prefix, user)
        .map(|mgr| Box::new(mgr) as Box<dyn ServiceManager>)
}

struct LaunchDServiceManager {
    name: OsString,
    prefix: OsString,
    user: bool,
}

impl LaunchDServiceManager {
    fn new(name: OsString, prefix: OsString, user: bool) -> UniResult<Self, ServiceErrKind> {
        // launchd? (must be a valid subcommand else returns exit code 1)
        if Self::launch_ctl("list", vec![]).is_ok() {
            Ok(Self { name, prefix, user })
        } else {
            Err(UniError::from_kind_context(
                ServiceErrKind::ServiceManagementNotAvailable,
                "launchd is not available on this system",
            ))
        }
    }

    pub fn launch_ctl(sub_cmd: &str, args: Vec<&OsStr>) -> UniResult<String, ServiceErrKind> {
        let mut command = Command::new(LAUNCH_CTL);

        command
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());

        if !sub_cmd.is_empty() {
            command.arg(sub_cmd);
        }

        for arg in args {
            command.arg(arg);
        }

        tracing::debug!("Executing command: {:?}", command);
        let output = command.output().kind(ServiceErrKind::Unknown)?;

        if output.status.success() {
            Ok(String::from_utf8(output.stdout).kind(ServiceErrKind::BadUtf8)?)
        } else {
            let msg = String::from_utf8(output.stderr).kind(ServiceErrKind::BadUtf8)?;
            Err(ServiceErrKind::BadExitStatus(output.status.code(), msg).into_error())
        }
    }

    fn path(&self) -> UniResult<PathBuf, ServiceErrKind> {
        if self.user {
            Ok(dirs::home_dir()
                .ok_or_else(|| ServiceErrKind::HomeDirectoryNotFound.into_error())?
                .join("Library")
                .join("LaunchAgents"))
        } else {
            Ok(PathBuf::from(GLOBAL_PATH))
        }
    }

    fn make_file_name(&self) -> UniResult<PathBuf, ServiceErrKind> {
        let mut filename = OsString::new();
        filename.push(&self.prefix);
        filename.push(&self.name);
        filename.push(".plist");

        Ok(self.path()?.join(filename))
    }

    fn domain_target(&self) -> OsString {
        if self.user {
            let uid = unsafe { libc::getuid() }.to_string();

            let mut s = OsString::new();
            s.push("gui/");
            s.push(uid);
            s
        } else {
            "system".into()
        }
    }

    fn make_service_target(&self, fully_qualified: bool) -> OsString {
        let mut s = if fully_qualified {
            let domain = self.domain_target();
            let mut s = OsString::with_capacity(domain.len() + self.prefix.len() + self.name.len());
            s.push(domain);
            s.push("/");
            s
        } else {
            OsString::with_capacity(self.prefix.len() + self.name.len())
        };

        s.push(&self.prefix);
        s.push(&self.name);
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
    ) -> UniResult<(), ServiceErrKind> {
        // Combine program and args into a single vector
        let mut new_args: Vec<OsString> = Vec::with_capacity(args.len() + 1);
        new_args.push(program.into());
        new_args.extend(args);

        // Convert each argument to a string and format it for the service file
        let mut args = Vec::with_capacity(new_args.len());
        for arg in new_args {
            let arg = arg
                .into_string()
                .map_err(|_| ServiceErrKind::BadUtf8.into_error())?;
            let arg = format!(r#"                <string>{arg}</string>"#);
            args.push(arg);
        }
        let args = args.join("\n");

        // Make the service target label
        let label = self
            .make_service_target(false)
            .into_string()
            .map_err(|_| ServiceErrKind::BadUtf8.into_error())?;

        let service = format!(
            r#"r#"<?xml version="1.0" encoding="UTF-8"?>
        <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
                "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        <plist version="1.0">
        <dict>
            <key>Label</key><string>{label}</string>
            <key>ProgramArguments</key>
            <array>
{args}
            </array>
            <key>RunAtLoad</key><false/>
        </dict>
        </plist>
"#,
        );
        // This seemed to work once - what is different? Hmm..
        // let service = r#"<?xml version="1.0" encoding="UTF-8"?>
        // <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
        //         "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
        // <plist version="1.0">
        // <dict>
        //     <key>Label</key><string>com.test.testbin</string>
        //     <key>ProgramArguments</key>
        //     <array>
        //         <string>/Users/scott/src/rust/uni_service/target/debug/test_bin</string>
        // <string>service</string>
        //     </array>
        //     <key>RunAtLoad</key><false/>
        // </dict>
        // </plist>
        // "#;

        // Create directories and install
        let path = self.path()?;
        fs::create_dir_all(&path).kind(ServiceErrKind::IoError)?;
        let file = self.make_file_name()?;
        write_file(&file, &service, SERVICE_PERMS)?;

        //Self::launch_ctl("enable", vec![self.make_name(true).as_ref()])?;
        Self::launch_ctl(
            "bootstrap",
            vec![self.domain_target().as_ref(), file.as_ref()],
        )?;
        Ok(())
    }

    fn uninstall(&self) -> UniResult<(), ServiceErrKind> {
        // First calculate file path and unload
        let file = self.make_file_name()?;
        Self::launch_ctl(
            "bootout",
            vec![self.domain_target().as_ref(), file.as_ref()],
        )?;
        //Self::launch_ctl("disable", vec![self.make_name(true).as_ref()])?;

        // ...then wipe service file
        fs::remove_file(file).kind(ServiceErrKind::IoError)?;
        Ok(())
    }

    fn start(&self) -> UniResult<(), ServiceErrKind> {
        Self::launch_ctl(
            "kickstart",
            vec![OsStr::new("-kp"), self.make_service_target(true).as_ref()],
        )?;
        Ok(())
    }

    fn stop(&self) -> UniResult<(), ServiceErrKind> {
        Self::launch_ctl(
            "kill",
            vec![
                OsStr::new("SIGTERM"),
                self.make_service_target(true).as_ref(),
            ],
        )?;
        Ok(())
    }

    fn status(&self) -> UniResult<ServiceStatus, ServiceErrKind> {
        match Self::launch_ctl("print", vec![self.make_service_target(true).as_ref()]) {
            Ok(status) => {
                if status.contains("state = running") {
                    Ok(ServiceStatus::Running)
                } else {
                    Ok(ServiceStatus::Stopped)
                }
            }
            Err(e) => match e.kind_ref() {
                // This seems to be the exit code for when the service is not installed
                // I am not 100% sure it is ONLY used for this purpose
                ServiceErrKind::BadExitStatus(code, _) if *code == Some(113) => {
                    Ok(ServiceStatus::NotInstalled)
                }
                _ => Err(e),
            },
        }
    }
}
