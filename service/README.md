# uni_service

[![Crate](https://img.shields.io/crates/v/uni_service)](https://crates.io/crates/uni_service)
[![Docs](https://docs.rs/uni_service/badge.svg)](https://docs.rs/uni_service)
[![Build](https://github.com/nu11ptr/uni_service/workflows/CI/badge.svg)](https://github.com/nu11ptr/uni_service/actions)
[![codecov](https://codecov.io/github/nu11ptr/uni_service/graph/badge.svg?token=WfG4hos7X5)](https://codecov.io/github/nu11ptr/uni_service)

A crate for for building cross platform OS services

## Install

```shell
cargo add uni_service
# or
cargo add uni_service -F tokio
```

## Features

* Portable cross platform services (Windows, macOS, Linux and other UNIX-like systems)
* A single user supplied function is all that is required
* Synchronous and asynchronous services (see `axum` example)
* Any service can be run interactively from the CLI or in service mode
* Works with the regular OS service manager, and pairs well with [`uni_service_manager`](https://github.com/nu11ptr/uni_service/tree/main/manager)
* Minimal dependencies
* No `unsafe`

## Example

The `hello_service` function below is the service, the rest is just boilerplate.

```rust,no_run
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
```

## Status

This is currently beta, however, I am using this myself, so it will become production quality at some point.

## Contributions

Contributions are welcome as long they align with my vision for this crate.
