use std::{
    error::Error,
    mem,
    sync::mpsc::{Receiver, Sender, channel},
    thread::{self, JoinHandle},
};

use axum::{Router, extract::State, routing::get};
use uni_service::{ServiceApp, run_service};

// *** AxumServer ***

struct AxumServer {
    receiver: Option<Receiver<()>>,
    is_service: bool,
}

impl AxumServer {
    fn new(receiver: Receiver<()>, is_service: bool) -> Self {
        Self {
            receiver: Some(receiver),
            is_service,
        }
    }

    #[tokio::main]
    async fn run_server(&mut self) -> uni_service::Result<()> {
        let app = Router::new()
            .route("/", get(Self::root))
            .with_state(self.is_service);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
        let receiver = mem::take(&mut self.receiver).ok_or("Receiver not found")?;

        println!("Serving on port 8000...");
        axum::serve(listener, app)
            .with_graceful_shutdown(Self::wait_for_shutdown(receiver))
            .await?;
        Ok(())
    }

    async fn root(State(is_service): State<bool>) -> &'static str {
        if is_service {
            "Hello, World! (service mode)"
        } else {
            "Hello, World! (interactive mode)"
        }
    }

    async fn wait_for_shutdown(receiver: Receiver<()>) {
        receiver.recv().expect("Could not receive shutdown signal");
        println!("Shutdown signal received. Shutting down...");
    }
}

// *** AxumService ***

struct AxumService {
    sender: Sender<()>,
    receiver: Option<Receiver<()>>,
    handle: Option<JoinHandle<uni_service::Result<()>>>,
    is_service: bool,
}

impl AxumService {
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

impl ServiceApp for AxumService {
    fn name(&self) -> &str {
        "axum_service"
    }

    fn start(&mut self) -> uni_service::Result<()> {
        println!("Starting Axum server...");

        let receiver = mem::take(&mut self.receiver).ok_or("Receiver not found")?;
        let is_service = self.is_service;

        self.handle = Some(thread::spawn(move || {
            AxumServer::new(receiver, is_service).run_server()
        }));
        Ok(())
    }

    fn stop(&mut self) -> uni_service::Result<()> {
        println!("Stopping Axum server...");

        self.sender.send(())?;

        let handle = mem::take(&mut self.handle);
        if let Some(handle) = handle {
            handle.join().map_err(|_| "Error joining thread")??;
        }

        println!("Axum server is shut down.");
        Ok(())
    }
}

// *** Main ***

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    let service_mode = match std::env::args().nth(1).as_deref() {
        Some("service") => true,
        _ => false,
    };

    run_service(AxumService::new(service_mode), service_mode)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
