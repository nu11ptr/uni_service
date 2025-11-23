use std::sync::mpsc::Receiver;

use uni_service::{BaseService, run_service};

fn hello_service(shutdown: Receiver<()>, is_service: bool) -> uni_service::Result<()> {
    if is_service {
        tracing::info!("Hello, World! (service mode)");
    } else {
        tracing::info!("Hello, World! (interactive mode)");
    }
    shutdown.recv()?;
    tracing::info!("Shutdown signal received. Shutting down...");
    Ok(())
}

fn run() -> uni_service::Result<()> {
    let service_mode = matches!(std::env::args().nth(1).as_deref(), Some("service"));
    let service = BaseService::new_sync("hello_world", hello_service, service_mode);

    run_service(service, service_mode)?;
    Ok(())
}

fn main() {
    tracing_subscriber::fmt().with_target(false).init();

    if let Err(e) = run() {
        tracing::error!("Error: {}", e);
        std::process::exit(1);
    }
}
