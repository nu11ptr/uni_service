# uni_service_manager

[![Crate](https://img.shields.io/crates/v/uni_service_manager)](https://crates.io/crates/uni_service_manager)
[![Docs](https://docs.rs/uni_service_manager/badge.svg)](https://docs.rs/uni_service_manager)
[![Build](https://github.com/nu11ptr/uni_service/workflows/CI/badge.svg)](https://github.com/nu11ptr/uni_service/actions)
[![codecov](https://codecov.io/github/nu11ptr/uni_service/graph/badge.svg?token=WfG4hos7X5)](https://codecov.io/github/nu11ptr/uni_service)

A crate for for managing cross platform OS services

## Install

```shell
cargo add uni_service_manager
```

## Features

* Cross platform (Windows, macOS/launchd, linux/systemd)
* Manage OS services in a platform agnostic manner
* Supports both user and system services (even on Windows)
* Works with any OS service, but pairs well with [`uni_service`](https://github.com/nu11ptr/uni_service)
* Minimal dependencies

## Example

Discover platform capabilities and then install, start, wait, stop and uninstall a service.

```rust,no_run
use std::{env, io, process};
use std::{io::Write as _, time::Duration};

use uni_service_manager::{ServiceCapabilities, ServiceSpec, UniServiceManager};

const TIMEOUT: Duration = Duration::from_secs(5);

fn main() {
    // Windows _user_ services require a new logon before they can be started, so we will use a system service on Windows
    let user = !UniServiceManager::capabilities()
        .contains(ServiceCapabilities::USER_SERVICES_REQUIRE_NEW_LOGON);
    let user_manager = UniServiceManager::new("my_service", "com.example.", user).unwrap();

    let spec = ServiceSpec::new("path/to/my/executable")
        .arg("my_arg").unwrap()
        .display_name("My display name").unwrap()
        .description("My awesome service").unwrap()
        .set_autostart()
        .set_restart_on_failure();

    user_manager.install_if_needed_and_start(&spec, TIMEOUT).unwrap();

    io::stdout().flush().unwrap();
    println!("Press Enter to stop the service...");
    let mut buffer = String::new();
    io::stdin().read_line(&mut buffer).unwrap();

    user_manager.stop_if_needed_and_uninstall(TIMEOUT).unwrap();
}
```

## Status

This is currently beta, however, I am using this myself, so it will become production quality at some point.

## Contributions

Contributions are welcome as long they align with my vision for this crate.

