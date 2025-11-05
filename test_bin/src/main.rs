use uni_service::{ServiceApp, run_service};

struct TestService {}

impl ServiceApp for TestService {
    fn name(&self) -> &str {
        "test_bin"
    }

    fn start(&mut self) -> uni_service::Result<()> {
        println!("Starting test service");
        Ok(())
    }

    fn stop(&mut self) -> uni_service::Result<()> {
        println!("Stopping test service");
        Ok(())
    }
}

fn main() {
    // Run as a service if the first argument is "service", else run in interactive mode
    let service_mode = match std::env::args().nth(1) {
        Some(arg) => arg == "service",
        None => false,
    };

    if service_mode {
        println!("Running as a service");
    } else {
        println!("Running in interactive mode");
    }

    if let Err(e) = run_service(Box::new(TestService {}), service_mode) {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
