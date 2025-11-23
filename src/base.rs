use std::{
    mem,
    sync::mpsc::{Receiver, channel},
    thread::{self, JoinHandle},
};

use crate::{Result, ServiceApp};

/// A base service implementation that can be used to build services.
pub struct BaseService<F, R> {
    name: String,
    service_fn: Option<F>,
    sender_fn: Box<dyn (Fn() -> Result<()>) + Send>,
    receiver: Option<R>,
    handle: Option<JoinHandle<Result<()>>>,
    is_service: bool,
}

impl<F, R> BaseService<F, R>
where
    F: FnOnce(R, bool) -> Result<()>,
{
    /// Creates a new base service with any custom sender/receiver pair (typically a channel). `service_fn`
    /// is the function that will be executed by the service. `sender_fn` is a function that will be called
    /// to notify the receiver it is time to shutdown the service. `receiver` is the receiver of the shutdown
    /// notification. The receiver is passed to the service function as a parameter.
    pub fn new(
        name: impl Into<String>,
        service_fn: F,
        is_service: bool,
        sender_fn: impl Fn() -> Result<()> + Send + 'static,
        receiver: R,
    ) -> Self {
        Self {
            name: name.into(),
            service_fn: Some(service_fn),
            sender_fn: Box::new(sender_fn),
            receiver: Some(receiver),
            handle: None,
            is_service,
        }
    }
}

impl<F> BaseService<F, Receiver<()>>
where
    F: FnOnce(Receiver<()>, bool) -> Result<()>,
{
    /// Creates a new base service for synchronous applications. `service_fn` is the function that will
    /// be executed by the service. A synchronous receiver channel will be passed to the service function
    /// and will receive a message when the service should shutdown.
    pub fn new_sync(name: impl Into<String>, service_fn: F, is_service: bool) -> Self {
        let (sender, receiver) = channel();
        let sender = move || {
            sender.send(())?;
            Ok(())
        };
        Self::new(name, service_fn, is_service, sender, receiver)
    }
}

#[cfg(feature = "tokio")]
impl<F> BaseService<F, tokio::sync::mpsc::Receiver<()>>
where
    F: FnOnce(tokio::sync::mpsc::Receiver<()>, bool) -> Result<()>,
{
    /// Creates a new base service for `tokio` asynchronous applications. `service_fn` is the function that will
    /// be executed by the service. An asynchronous receiver channel will be passed to the service function
    /// and will receive a message when the service should shutdown.
    pub fn new_tokio(name: impl Into<String>, service_fn: F, is_service: bool) -> Self {
        let (sender, receiver) = tokio::sync::mpsc::channel(1);
        let sender = move || {
            sender.blocking_send(())?;
            Ok(())
        };
        Self::new(name, service_fn, is_service, sender, receiver)
    }
}

impl<F, R> ServiceApp for BaseService<F, R>
where
    F: FnOnce(R, bool) -> Result<()> + Send + 'static,
    R: Send + 'static,
{
    fn name(&self) -> &str {
        &self.name
    }

    fn start(&mut self) -> Result<()> {
        tracing::info!("Starting service '{}'...", self.name);

        let receiver = mem::take(&mut self.receiver)
            .ok_or("Receiver not found (service might have been started twice)")?;
        let is_service = self.is_service;
        let service_fn = mem::take(&mut self.service_fn).ok_or("Service function not found")?;

        self.handle = Some(thread::spawn(move || service_fn(receiver, is_service)));
        Ok(())
    }

    fn stop(mut self: Box<Self>) -> Result<()> {
        match mem::take(&mut self.handle) {
            Some(handle) if handle.is_finished() => {
                tracing::warn!(
                    "Service '{}' was already stopped (before we signalled it to do so).",
                    self.name
                );
                Ok(())
            }
            Some(handle) => {
                tracing::info!("Stopping service '{}'...", self.name);
                (self.sender_fn)()?;

                handle.join().map_err(|_| "Error joining thread")??;

                tracing::info!("Service '{}' is shut down.", self.name);
                Ok(())
            }
            None => Err(format!("Thread handle not found for service '{}'.", self.name).into()),
        }
    }
}
