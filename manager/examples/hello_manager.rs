use std::{io::Write as _, time::Duration};

use uni_service_manager::{ServiceCapabilities, ServiceSpec, ServiceStatus, UniServiceManager};

const TIMEOUT: Duration = Duration::from_secs(5);

fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Windows user services require a new logon before they can be started, so we will use a system service on Windows.
    // NOTE: Windows services always require elevated privileges to install, so run this example as administrator.
    let user = !UniServiceManager::capabilities()
        .contains(ServiceCapabilities::USER_SERVICES_REQUIRE_NEW_LOGON);
    let user_manager = UniServiceManager::new("hello_world", "com.example.", user)?;
    let spec = ServiceSpec::new("hello_world")
        .display_name("Hello World")?
        .description("Hello World service")?;

    print!("Installing service...");
    user_manager.install(&spec)?;
    user_manager.wait_for_status(ServiceStatus::Stopped, TIMEOUT)?;
    println!("done");

    print!("Starting service...");
    user_manager.start()?;
    user_manager.wait_for_status(ServiceStatus::Running, TIMEOUT)?;
    println!("done");

    std::io::stdout().flush()?;
    println!("Press Enter to stop the service...");
    let mut buffer = String::new();
    std::io::stdin().read_line(&mut buffer)?;

    print!("Stopping service...");
    user_manager.stop()?;
    user_manager.wait_for_status(ServiceStatus::Stopped, TIMEOUT)?;
    println!("done");

    print!("Uninstalling service...");
    user_manager.uninstall()?;
    user_manager.wait_for_status(ServiceStatus::NotInstalled, TIMEOUT)?;
    println!("done");

    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
