use std::{env, io, process};
use std::{io::Write as _, time::Duration};

use uni_service_manager::{ServiceCapabilities, ServiceSpec, ServiceStatus, UniServiceManager};

const TIMEOUT: Duration = Duration::from_secs(5);

fn run(
    service_name: &str,
    display_name: &str,
    description: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Windows user services require a new logon before they can be started, so we will use a system service on Windows.
    // NOTE: Windows services always require elevated privileges to install, so run this example as administrator.
    let user = !UniServiceManager::capabilities()
        .contains(ServiceCapabilities::USER_SERVICES_REQUIRE_NEW_LOGON);
    let user_manager = UniServiceManager::new(service_name, "com.example.", user)?;

    let mut bin_path = std::env::current_exe().unwrap();
    bin_path.set_file_name(service_name);

    if !bin_path.exists() {
        return Err(format!(
            "Executable not found: {}. Make sure to build the service examples first.",
            bin_path.display()
        )
        .into());
    }

    let spec = ServiceSpec::new(bin_path)
        .arg("service")?
        .display_name(display_name)?
        .description(description)?;

    print!("Installing service '{service_name}'...",);
    user_manager.install(&spec)?;
    user_manager.wait_for_status(ServiceStatus::Stopped, TIMEOUT)?;
    println!("done");

    print!("Starting service '{service_name}'...");
    user_manager.start()?;
    user_manager.wait_for_status(ServiceStatus::Running, TIMEOUT)?;
    println!("done");

    io::stdout().flush()?;
    println!("Press Enter to stop the service...");
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer)?;

    print!("Stopping service '{service_name}'...");
    user_manager.stop()?;
    user_manager.wait_for_status(ServiceStatus::Stopped, TIMEOUT)?;
    println!("done");

    print!("Uninstalling service '{service_name}'...");
    user_manager.uninstall()?;
    user_manager.wait_for_status(ServiceStatus::NotInstalled, TIMEOUT)?;
    println!("done");

    Ok(())
}

fn main() {
    if env::args().len() < 2 {
        eprintln!("Usage: manager <service_name>");
        process::exit(1);
    }
    let service_name = env::args().nth(1).unwrap();

    let (display_name, description) = match service_name.as_str() {
        "axum" => ("Axum Service", "Axum service"),
        "hello_world" => ("Hello World", "Hello World service"),
        _ => {
            eprintln!("Unknown service: {}", service_name);
            process::exit(1);
        }
    };

    if let Err(e) = run(&service_name, display_name, description) {
        eprintln!("Error: {}", e);
        process::exit(1);
    }
}
