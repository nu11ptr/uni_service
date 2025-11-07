use polling::{Event, Events, Poller};
use std::{
    io::{self, Read as _},
    net::{TcpListener, TcpStream},
    time::Duration,
};

pub struct TcpServer {
    listener: TcpListener,
    socket: Option<TcpStream>,
    poller: Poller,
    listener_key: usize,
    socket_key: usize,
}

impl TcpServer {
    pub fn new(address: &str) -> io::Result<Self> {
        let socket = TcpListener::bind(address)?;
        socket.set_nonblocking(true)?;
        let key = 1;

        let poller = Poller::new()?;
        unsafe { poller.add(&socket, Event::readable(key))? };

        Ok(Self {
            listener: socket,
            socket: None,
            poller,
            listener_key: key,
            socket_key: 2,
        })
    }
}

impl TcpServer {
    pub fn wait_for_connection(&mut self, timeout: Duration) -> io::Result<()> {
        let mut events = Events::new();

        self.poller.wait(&mut events, Some(timeout))?;
        for ev in events.iter() {
            if ev.key == self.listener_key {
                let (socket, _) = self.listener.accept()?;
                unsafe { self.poller.add(&socket, Event::readable(self.socket_key))? };
                socket.set_nonblocking(true)?;
                self.socket = Some(socket);
                return Ok(());
            }
        }

        Err(io::Error::new(io::ErrorKind::Other, "No event found"))
    }

    pub fn expect_message(&mut self, message: &str, timeout: Duration) -> io::Result<()> {
        if let Some(socket) = &mut self.socket.as_ref() {
            let mut events = Events::new();
            self.poller.wait(&mut events, Some(timeout))?;

            for ev in events.iter() {
                if ev.key == self.socket_key {
                    let mut buffer = vec![0; message.len()];
                    let n = socket.read(&mut buffer)?;
                    if n == 0 {
                        return Err(io::Error::new(io::ErrorKind::Other, "Socket closed"));
                    }
                    self.poller
                        .modify(socket, Event::readable(self.socket_key))?;

                    let received_message = String::from_utf8_lossy(&buffer[..n]);
                    return if message == received_message {
                        Ok(())
                    } else {
                        Err(io::Error::new(
                            io::ErrorKind::Other,
                            format!(
                                "Message mismatch: expected '{}', got '{}'",
                                message, received_message
                            ),
                        ))
                    };
                }
            }

            Err(io::Error::new(io::ErrorKind::Other, "No event found"))
        } else {
            Err(io::Error::new(io::ErrorKind::Other, "No socket found"))
        }
    }
}
