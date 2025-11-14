use std::sync::mpsc::Receiver;

use uni_service::{BaseService, run_service};

fn hello_service(shutdown: Receiver<()>, is_service: bool) -> uni_service::Result<()> {
    if is_service {
        println!("Hello, World! (service mode)");
    } else {
        println!("Hello, World! (interactive mode)");
    }
    shutdown.recv()?;
    println!("Shutdown signal received. Shutting down...");
    Ok(())
}

fn run() -> uni_service::Result<()> {
    let service_mode = match std::env::args().nth(1).as_deref() {
        Some("service") => true,
        _ => false,
    };
    let service = BaseService::new_sync("hello_world", hello_service, service_mode);

    run_service(service, service_mode)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
