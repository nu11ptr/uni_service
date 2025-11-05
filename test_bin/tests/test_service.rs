use std::sync::OnceLock;

use uni_service::{ServiceManager, ServiceStatus, new_service_manager};

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

fn service_stopped_or_err(manager: &dyn ServiceManager) -> bool {
    #[cfg(not(target_os = "linux"))]
    return manager.status().is_err();
    // Systemd doesn't indicate if the service is present, just reports "inactive" with exit code 3
    #[cfg(target_os = "linux")]
    return manager.status().unwrap() == ServiceStatus::Stopped;
}

#[test]
fn test_service() {
    init_tracing();

    // Cargo sets this env var to the path of the built executable
    let bin_path = env!("CARGO_BIN_EXE_test_bin");

    let manager = new_service_manager("test_bin", "org.test.", true).unwrap();
    assert!(service_stopped_or_err(manager.as_ref()));

    manager
        .install(
            bin_path.into(),
            vec!["service".into()],
            "Test service".into(),
            "Test service description".into(),
        )
        .unwrap();
    assert_eq!(manager.status().unwrap(), ServiceStatus::Stopped);

    manager.start().unwrap();
    assert_eq!(manager.status().unwrap(), ServiceStatus::Running);

    manager.stop().unwrap();
    assert_eq!(manager.status().unwrap(), ServiceStatus::Stopped);

    manager.uninstall().unwrap();
    assert!(service_stopped_or_err(manager.as_ref()));
}
