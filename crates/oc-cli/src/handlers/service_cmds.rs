//! `opencode service *` (handlers/service/*) — background daemon control
//! and the service config get/set/unset surface.

use crate::cli::ServiceCommands;
use crate::services::{server_connection, service_config};

pub async fn run(command: &ServiceCommands) -> i32 {
    match command {
        ServiceCommands::Start => start().await,
        ServiceCommands::Restart => restart().await,
        ServiceCommands::Status => status().await,
        ServiceCommands::Stop => stop().await,
        ServiceCommands::Get { key, name } => get(key.as_deref(), name.as_deref()),
        ServiceCommands::Set { key, value, nested_value } => {
            set(key, value, nested_value.as_deref()).await
        }
        ServiceCommands::Unset { key, name } => unset(key, name.as_deref()).await,
    }
}

async fn start() -> i32 {
    match server_connection::ensure_managed(false).await {
        Ok(endpoint) => {
            println!("{}", endpoint.url);
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

async fn restart() -> i32 {
    let options = service_config::options(false);
    // Explicit: automatic service replacement would lose terminals —
    // a restart shuts them down deliberately.
    server_connection::shutdown_persistent_pty().await;
    oc_server::lifecycle::stop(oc_server::lifecycle::StopOptions {
        file: options.file.clone(),
        pty: oc_server::lifecycle::PtyStop::Clear,
        timing: options.timing,
    })
    .await;
    match server_connection::ensure_managed(false).await {
        Ok(endpoint) => {
            println!("{}", endpoint.url);
            0
        }
        Err(message) => {
            eprintln!("{message}");
            1
        }
    }
}

async fn status() -> i32 {
    if let Some(endpoint) = server_connection::discover_managed(None).await {
        println!("{}", endpoint.url);
        0
    } else {
        println!("stopped");
        0
    }
}

async fn stop() -> i32 {
    server_connection::shutdown_persistent_pty().await;
    let options = service_config::options(false);
    oc_server::lifecycle::stop(oc_server::lifecycle::StopOptions {
        file: options.file,
        pty: oc_server::lifecycle::PtyStop::Clear,
        timing: options.timing,
    })
    .await;
    0
}

fn get(key: Option<&str>, name: Option<&str>) -> i32 {
    match service_config::get(key, name) {
        Ok(value) => {
            println!("{value}");
            0
        }
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}

async fn set(key: &str, value: &str, nested_value: Option<&str>) -> i32 {
    match service_config::set(key, value, nested_value).await {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}

async fn unset(key: &str, name: Option<&str>) -> i32 {
    match service_config::unset(key, name).await {
        Ok(()) => 0,
        Err(message) => {
            eprintln!("Error: {message}");
            1
        }
    }
}
