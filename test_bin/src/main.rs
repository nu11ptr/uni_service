use std::{
    error::Error,
    io::{self, Write as _},
    mem,
    net::{SocketAddr, TcpStream},
    sync::{
        Arc, Mutex,
        mpsc::{Receiver, Sender, channel},
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use uni_service::{ServiceApp, run_service};

const SOCKET_TIMEOUT: Duration = Duration::from_secs(3);

struct TcpClient {
    socket: TcpStream,
}

impl TcpClient {
    fn connect(address: &str, timeout: Duration) -> io::Result<Self> {
        let address = address
            .parse::<SocketAddr>()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
        let socket = TcpStream::connect_timeout(&address, timeout)?;
        socket.set_read_timeout(Some(timeout))?;
        socket.set_write_timeout(Some(timeout))?;
        Ok(Self { socket: socket })
    }

    fn send_message(&mut self, message: &str) -> io::Result<()> {
        self.socket.write_all(message.as_bytes())?;
        Ok(())
    }
}

struct TestService {
    service_mode: bool,
    handle: Option<JoinHandle<io::Result<()>>>,
    sender: Sender<()>,
    receiver: Option<Receiver<()>>,
    client: Option<Arc<Mutex<TcpClient>>>,
}

impl TestService {
    fn new(service_mode: bool, client: Option<TcpClient>) -> Self {
        let (sender, receiver) = channel();
        Self {
            service_mode,
            handle: None,
            sender,
            receiver: Some(receiver),
            client: client.map(|c| Arc::new(Mutex::new(c))),
        }
    }

    fn send_message(
        client: Option<&Arc<Mutex<TcpClient>>>,
        sock_msg: &str,
        print_msg: &str,
    ) -> io::Result<()> {
        if let Some(client) = client {
            client
                .lock()
                .expect("Mutex poisoned")
                .send_message(sock_msg)
        } else {
            println!("{}", print_msg);
            Ok(())
        }
    }

    fn hello(&mut self) -> io::Result<()> {
        if self.service_mode {
            Self::send_message(self.client.as_ref(), "service", "Running as a service")?;
        } else {
            Self::send_message(
                self.client.as_ref(),
                "regular",
                "Running in interactive mode",
            )?;
        }

        Ok(())
    }

    fn goodbye(&mut self) -> io::Result<()> {
        Self::send_message(self.client.as_ref(), "goodbye", "Goodbye!")
    }

    fn start_thread(&mut self) {
        let client = self.client.clone();
        let receiver = mem::take(&mut self.receiver);

        self.handle = Some(thread::spawn(move || {
            Self::send_message(client.as_ref(), "running", "Service is running")?;

            match receiver.as_ref() {
                Some(receiver) => match receiver.recv() {
                    Ok(_) => Self::send_message(client.as_ref(), "quitting", "Shutting down..."),
                    Err(e) => {
                        eprintln!("Error receiving message: {}", e);
                        Err(io::Error::new(io::ErrorKind::Other, e))
                    }
                },
                None => Ok(()),
            }
        }));
    }
}

impl ServiceApp for TestService {
    fn name(&self) -> &str {
        "test_bin"
    }

    fn start(&mut self) -> uni_service::Result<()> {
        Self::send_message(self.client.as_ref(), "starting", "Startup requested")?;

        self.start_thread();

        Ok(())
    }

    fn stop(mut self: Box<Self>) -> uni_service::Result<()> {
        Self::send_message(self.client.as_ref(), "stopping", "Shutdown requested")?;

        self.sender.send(())?;

        let handle = mem::take(&mut self.handle);
        if let Some(handle) = handle {
            handle.join().unwrap().unwrap();
        }

        Ok(())
    }
}

impl Drop for TestService {
    fn drop(&mut self) {
        if let Err(e) = self.goodbye() {
            eprintln!("Error: {}", e);
        }
    }
}

fn run() -> Result<(), Box<dyn Error + Send + Sync>> {
    // Run as a service if the first argument is "service", else run in interactive mode
    let (service_mode, next_arg) = match std::env::args().nth(1).as_deref() {
        Some("service") => (true, 2),
        _ => (false, 1),
    };
    // If the next argument is present, it's the address of the TCP server
    let client = match std::env::args().nth(next_arg).as_deref() {
        Some(address) => Some(TcpClient::connect(address, SOCKET_TIMEOUT)?),
        None => None,
    };

    let mut service = TestService::new(service_mode, client);
    service.hello()?;

    run_service(service, service_mode)?;
    Ok(())
}

fn main() {
    if let Err(e) = run() {
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
