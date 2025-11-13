use std::{
    mem,
    sync::mpsc::{Receiver, Sender, channel},
    thread::{self, JoinHandle},
};

use crate::{Result, ServiceApp};

/// A base service implementation that can be used to build services.
pub struct BaseService<F> {
    name: String,
    service_fn: Option<F>,
    sender: Sender<()>,
    receiver: Option<Receiver<()>>,
    handle: Option<JoinHandle<Result<()>>>,
    is_service: bool,
}

impl<F> BaseService<F>
where
    F: FnOnce(Receiver<()>, bool) -> Result<()>,
{
    /// Creates a new base service.
    pub fn new(name: impl Into<String>, service_fn: F, is_service: bool) -> Self {
        let (sender, receiver) = channel();

        Self {
            name: name.into(),
            service_fn: Some(service_fn),
            sender,
            receiver: Some(receiver),
            handle: None,
            is_service,
        }
    }
}

impl<F> ServiceApp for BaseService<F>
where
    F: FnOnce(Receiver<()>, bool) -> Result<()> + Send + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<()> {
        println!("Starting service...");

        let receiver = mem::take(&mut self.receiver).ok_or("Receiver not found")?;
        let is_service = self.is_service;
        let service_fn = mem::take(&mut self.service_fn).ok_or("Service function not found")?;

        self.handle = Some(thread::spawn(move || service_fn(receiver, is_service)));
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        println!("Stopping service...");

        self.sender.send(())?;

        let handle = mem::take(&mut self.handle);
        if let Some(handle) = handle {
            handle.join().map_err(|_| "Error joining thread")??;
        }

        println!("Service is shut down.");
        Ok(())
    }
}
