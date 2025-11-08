mod common;

use std::{process::Command, sync::OnceLock, thread, time::Duration};

use send_ctrlc::{Interruptible as _, InterruptibleCommand as _};
use uni_service_manager::{ServiceStatus, UniServiceManager};

use crate::common::TcpServer;

const TIMEOUT: Duration = Duration::from_secs(3);

static TRACING: OnceLock<()> = OnceLock::new();

fn init_tracing() {
    TRACING.get_or_init(|| {
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .with_target(false)
            .with_test_writer() // ensures logs go to test output
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

#[test]
fn test_service() {
    const SERVER_ADDRESS: &str = "127.0.0.1:53165";
    init_tracing();

    // Cargo sets this env var to the path of the built executable
    let bin_path = env!("CARGO_BIN_EXE_test_bin");

    let manager = UniServiceManager::new("test_bin", "org.test.", true).unwrap();
    manager
        .wait_for_status(ServiceStatus::NotInstalled, TIMEOUT)
        .unwrap();

    manager
        .install(
            bin_path.into(),
            vec!["service".into(), SERVER_ADDRESS.into()],
            "Test service".into(),
            "Test service description".into(),
        )
        .unwrap();
    manager
        .wait_for_status(ServiceStatus::Stopped, TIMEOUT)
        .unwrap();

    let handle = thread::spawn(move || {
        let mut server = TcpServer::new(SERVER_ADDRESS).unwrap();
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
    manager.stop().unwrap();
    manager
        .wait_for_status(ServiceStatus::Stopped, TIMEOUT)
        .unwrap();
    handle.join().unwrap();
    // NOTE: It is not possible to get the goodbye message because the service is stopped before the message is sent

    manager.uninstall().unwrap();
    manager
        .wait_for_status(ServiceStatus::NotInstalled, TIMEOUT)
        .unwrap();
}
