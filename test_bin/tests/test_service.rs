mod common;

use std::{process::Command, sync::OnceLock, thread, time::Duration};

use send_ctrlc::{Interruptible as _, InterruptibleCommand as _};
use uni_service_manager::{ServiceCapabilities, ServiceSpec, ServiceStatus, UniServiceManager};

use crate::common::TcpServer;

const TIMEOUT: Duration = Duration::from_secs(3);

static TRACING: OnceLock<()> = OnceLock::new();

fn init_tracing() {
    TRACING.get_or_init(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::WARN)
            .with_target(false)
            .init()
    });
}

#[test]
fn test_service_interactive() {
    const SERVER_ADDRESS: &str = "127.0.0.1:53164";
    init_tracing();

    let bin_path = env!("CARGO_BIN_EXE_test_bin");
    let mut command = Command::new(bin_path)
        .arg(SERVER_ADDRESS)
        .spawn_interruptible()
        .unwrap();

    let mut server = TcpServer::new(SERVER_ADDRESS).unwrap();
    server.wait_for_connection(TIMEOUT).unwrap();
    server.expect_message("regular", TIMEOUT).unwrap();
    server.expect_message("starting", TIMEOUT).unwrap();
    server.expect_message("running", TIMEOUT).unwrap();

    command.interrupt().unwrap();
    server.expect_message("stopping", TIMEOUT).unwrap();
    server.expect_message("quitting", TIMEOUT).unwrap();
    server.expect_message("goodbye", TIMEOUT).unwrap();
    command.wait().unwrap();
}

#[derive(Clone, Copy)]
enum MultiPhase {
    NotMultiPhase,
    #[allow(dead_code)]
    ExpectingEitherPhase,
    #[allow(dead_code)]
    ExpectingPhase2,
}

impl MultiPhase {
    fn is_multi_phase(self) -> bool {
        matches!(
            self,
            MultiPhase::ExpectingEitherPhase | MultiPhase::ExpectingPhase2
        )
    }
}

fn test_service(name: &str, bind_address: &'static str, user: bool, multi_phase: MultiPhase) {
    // Cargo sets this env var to the path of the built executable
    let bin_path = env!("CARGO_BIN_EXE_test_bin");

    let manager = UniServiceManager::new(name, "org.test.", user).unwrap();
    let installed = match (manager.status().unwrap(), multi_phase) {
        (ServiceStatus::NotInstalled, MultiPhase::ExpectingPhase2) => {
            tracing::warn!(
                "MULTI_PHASE_1: Skipping multi-phase test because not running as administrator"
            );
            return;
        }
        (ServiceStatus::NotInstalled, _) => false,
        _ if multi_phase.is_multi_phase() => true,
        _ => panic!("Service is already installed"),
    };

    let ready_to_start = installed
        || !user
        || !UniServiceManager::capabilities()
            .contains(ServiceCapabilities::USER_SERVICES_REQUIRE_NEW_LOGON);

    if !installed {
        if multi_phase.is_multi_phase() {
            tracing::warn!(
                "MULTI_PHASE_1: Installing service only, execution deferred until after next logon"
            );
        }

        let spec = ServiceSpec::new(bin_path)
            .arg("service")
            .unwrap()
            .arg(bind_address)
            .unwrap()
            .display_name("Test service")
            .unwrap()
            .description("Test service description")
            .unwrap();

        manager.install(&spec).unwrap();
    } else {
        tracing::warn!(
            "MULTI_PHASE_2: Skipping service installation because it is already installed"
        );
    }

    manager
        .wait_for_status(ServiceStatus::Stopped, TIMEOUT)
        .unwrap();

    if ready_to_start {
        if multi_phase.is_multi_phase() {
            tracing::warn!("MULTI_PHASE_2: Executing service");
        }

        let handle = thread::spawn(move || {
            let mut server = TcpServer::new(bind_address).unwrap();
            server.wait_for_connection(TIMEOUT).unwrap();
            server
        });
        manager.start().unwrap();

        let mut server = handle.join().unwrap();
        server.expect_message("service", TIMEOUT).unwrap();
        server.expect_message("starting", TIMEOUT).unwrap();
        server.expect_message("running", TIMEOUT).unwrap();
        manager
            .wait_for_status(ServiceStatus::Running, TIMEOUT)
            .unwrap();

        let handle = thread::spawn(move || {
            server.expect_message("stopping", TIMEOUT).unwrap();
            server.expect_message("quitting", TIMEOUT).unwrap();
        });
        manager.stop_and_wait(TIMEOUT).unwrap();
        handle.join().unwrap();
        // NOTE: It is not possible to get the goodbye message because the service is stopped before the message is sent
    } else {
        if multi_phase.is_multi_phase() {
            tracing::warn!("MULTI_PHASE_1: Service execution deferred until after next logon");
        } else {
            tracing::warn!(
                "Skipping service execution because this is a user service that requires a new logon"
            );
        }
    }

    if !multi_phase.is_multi_phase() || installed {
        manager.uninstall_and_wait(TIMEOUT).unwrap();
    }
}

#[cfg(windows)]
fn is_admin() -> bool {
    use std::process::Stdio;

    let output = Command::new("net")
        .arg("session")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .unwrap();
    output.status.success()
}

// Requires administrator privileges to install/uninstall, and can only be started/stopped on logon/logoff
#[cfg(windows)]
#[test]
fn test_windows_user_service() {
    init_tracing();

    if std::env::var("MULTI_PHASE").is_err() {
        test_service(
            "user_test",
            "127.0.0.1:53165",
            true,
            MultiPhase::NotMultiPhase,
        );
    } else {
        tracing::warn!(
            "Skipping 'test_windows_user_service' because 'MULTI_PHASE' environment variable is set"
        );
    }
}

// Requires administrator privileges to install, and can only be started/stopped after logon/logoff
// (at which point our user can start/stop/uninstall)
#[cfg(windows)]
#[test]
fn test_windows_user_service_multi_phase() {
    init_tracing();

    if std::env::var("MULTI_PHASE").is_ok() {
        let multi_phase = match is_admin() {
            true => MultiPhase::ExpectingEitherPhase,
            false => MultiPhase::ExpectingPhase2,
        };
        test_service(
            "user_test_multi_phase",
            "127.0.0.1:53165",
            true,
            multi_phase,
        );
    } else {
        tracing::warn!(
            "Skipping 'test_windows_user_service_multi_phase' because 'MULTI_PHASE' environment variable is not set"
        );
    }
}

// Regular user can install/uninstall
#[cfg(windows)]
#[test]
fn test_windows_system_service() {
    init_tracing();

    if is_admin() {
        test_service(
            "system_test",
            "127.0.0.1:53166",
            false,
            MultiPhase::NotMultiPhase,
        );
    } else {
        tracing::warn!(
            "Skipping 'test_windows_system_service' because not running as administrator"
        );
    }
}

#[cfg(not(windows))]
fn is_root() -> bool {
    unsafe { libc::getuid() == 0 }
}

// Regular user can install/uninstall
#[cfg(not(windows))]
#[test]
fn test_unix_user_service() {
    init_tracing();
    if !is_root() {
        test_service(
            "user_test",
            "127.0.0.1:53165",
            true,
            MultiPhase::NotMultiPhase,
        );
    } else {
        tracing::warn!("Skipping 'test_unix_user_service' because not running as user");
    }
}

// Requires root to install/uninstall
#[cfg(not(windows))]
#[test]
fn test_unix_system_service() {
    init_tracing();
    if is_root() {
        test_service(
            "system_test",
            "127.0.0.1:53166",
            false,
            MultiPhase::NotMultiPhase,
        );
    } else {
        tracing::warn!("Skipping 'test_unix_system_service' because not running as root");
    }
}
