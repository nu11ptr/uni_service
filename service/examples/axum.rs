use std::sync::mpsc::Receiver;

use axum::{Router, extract::State, routing::get};

use uni_service::{BaseService, run_service};

// *** AxumServer ***

struct AxumServer {
    shutdown: Option<Receiver<()>>,
    is_service: bool,
}

impl AxumServer {
    fn new(receiver: Receiver<()>, is_service: bool) -> Self {
        Self {
            shutdown: Some(receiver),
            is_service,
        }
    }

    #[tokio::main(flavor = "current_thread")]
    async fn run_server(&mut self) -> uni_service::Result<()> {
        let app = Router::new()
            .route("/", get(Self::root))
            .with_state(self.is_service);

        let listener = tokio::net::TcpListener::bind("0.0.0.0:8000").await.unwrap();
        let receiver = std::mem::take(&mut self.shutdown).ok_or("Receiver not found")?;

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
        tokio::task::spawn_blocking(move || receiver.recv())
            .await
            .expect("Error executing spawned blocking task")
            .expect("Could not receive shutdown signal");
        println!("Shutdown signal received. Shutting down...");
    }
}

// *** Main ***

fn run() -> uni_service::Result<()> {
    let service_mode = match std::env::args().nth(1).as_deref() {
        Some("service") => true,
        _ => false,
    };

    let axum_service = |shutdown: Receiver<()>, is_service: bool| -> uni_service::Result<()> {
        let mut server = AxumServer::new(shutdown, is_service);
        server.run_server()
    };
    let service = BaseService::new("axum_service", axum_service, service_mode);
    run_service(service, service_mode)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
