use std::{
    error::Error,
    mem,
    sync::mpsc::{Receiver, Sender, channel},
    thread::{self, JoinHandle},
};

use axum::{Router, routing::get};
use uni_service::{ServiceApp, run_service};

async fn root() -> &'static str {
    "Hello, World!"
}

async fn wait_for_shutdown(receiver: Receiver<()>) {
    receiver.recv().expect("Could not receive shutdown signal");
}

struct AxumService {
    sender: Sender<()>,
    receiver: Option<Receiver<()>>,
    handle: Option<JoinHandle<uni_service::Result<()>>>,
}

impl AxumService {
    fn new() -> Self {
        let (sender, receiver) = channel();

        Self {
            sender,
            receiver: Some(receiver),
            handle: None,
        }
    }

    #[tokio::main]
    async fn run_server(receiver: Receiver<()>) -> uni_service::Result<()> {
        let app = Router::new().route("/", get(root));

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
        axum::serve(listener, app)
            .with_graceful_shutdown(wait_for_shutdown(receiver))
            .await?;
        Ok(())
    }
}

impl ServiceApp for AxumService {
    fn name(&self) -> &str {
        "axum_service"
    }

    fn start(&mut self) -> uni_service::Result<()> {
        let receiver = mem::take(&mut self.receiver).ok_or("Receiver not found")?;
        self.handle = Some(thread::spawn(move || Self::run_server(receiver)));
        Ok(())
    }

    fn stop(&mut self) -> uni_service::Result<()> {
        self.sender.send(())?;

        let handle = mem::take(&mut self.handle);
        if let Some(handle) = handle {
            handle.join().map_err(|_| "Error joining thread")??;
        }

        Ok(())
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Run as a service if the first argument is "service", else run in interactive mode
    let service_mode = match std::env::args().nth(1).as_deref() {
        Some("service") => true,
        _ => false,
    };

    run_service(AxumService::new(), service_mode)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
