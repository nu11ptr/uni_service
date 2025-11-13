use std::{
    error::Error,
    mem,
    sync::mpsc::{Receiver, Sender, channel},
    thread::{self, JoinHandle},
};

use uni_service::{ServiceApp, run_service};

// *** HelloService ***

struct HelloService {
    sender: Sender<()>,
    receiver: Option<Receiver<()>>,
    handle: Option<JoinHandle<uni_service::Result<()>>>,
    is_service: bool,
}

impl HelloService {
    fn new(is_service: bool) -> Self {
        let (sender, receiver) = channel();

        Self {
            sender,
            receiver: Some(receiver),
            handle: None,
            is_service,
        }
    }
}

impl ServiceApp for HelloService {
    fn name(&self) -> &str {
        "hello_service"
    }

    fn start(&mut self) -> uni_service::Result<()> {
        println!("Starting hello service...");

        let receiver = mem::take(&mut self.receiver).ok_or("Receiver not found")?;
        let is_service = self.is_service;

        self.handle = Some(thread::spawn(move || {
            if is_service {
                println!("Hello, World! (service mode)");
            } else {
                println!("Hello, World! (interactive mode)");
            }
            receiver.recv()?;
            println!("Shutdown signal received. Shutting down...");
            Ok(())
        }));
        Ok(())
    }

    fn stop(&mut self) -> uni_service::Result<()> {
        println!("Stopping hello service...");

        self.sender.send(())?;

        let handle = mem::take(&mut self.handle);
        if let Some(handle) = handle {
            handle.join().map_err(|_| "Error joining thread")??;
        }

        println!("Hello service is shut down.");
        Ok(())
    }
}

// *** Main ***

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let service_mode = match std::env::args().nth(1).as_deref() {
        Some("service") => true,
        _ => false,
    };

    run_service(HelloService::new(service_mode), service_mode)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
