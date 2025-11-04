use uni_service::{ServiceStatus, new_service_manager};

#[test]
fn test_service() {
    // Cargo sets this env var to the path of the built executable
    let bin_path = env!("CARGO_BIN_EXE_test_bin");

    let manager = new_service_manager("test_bin", "org.test.", true).unwrap();
    assert!(manager.status().is_err());

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
    assert!(manager.status().is_err());
}
