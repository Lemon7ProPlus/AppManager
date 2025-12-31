// src/main.rs

#![windows_subsystem = "windows"]

mod api;
mod manager;
mod service;

use api::AppState;
use manager::ServiceManager;

use clap::Parser;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{Mutex, mpsc};
use tower_http::cors::CorsLayer; 

/// Derive for clap
#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    #[arg(long)]
    debug: bool,
    #[arg(long)]
    listen: Option<String>,
}
/// Optimize memory usage
/// "current_thread" mod
#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    // process "--debug" command and open debug window
    if args.debug {
        #[cfg(windows)]
        unsafe {
            use windows_sys::Win32::System::Console::{
                ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
            };
            if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
                AllocConsole();
            }
        }
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::DEBUG)
            .init();
    }
    // Locate and initial config
    let config_path = "services.yaml";
    let mut manager = ServiceManager::new(config_path)?;

    // Autorun processing
    let auto_start_ids: Vec<String> = manager
        .services
        .values()
        .filter(|svc| svc.config.autorun.unwrap_or(false))
        .map(|svc| svc.config.id.clone())
        .collect();
    for id in auto_start_ids {
        let _ = manager.start(&id).await;
    }
    // get keep alive interval
    let keep_alive_seconds = manager.keep_alive_interval;
    // get listen address, default: 127.0.0.1:3000
    let listen_addr = args
        .listen
        .or(manager.config_listen.clone())
        .unwrap_or_else(|| "127.0.0.1:3000".to_string());
    // Create mpsc channel to process state and exit
    let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
    let shared_manager = Arc::new(Mutex::new(manager));
    let monitor_manager = shared_manager.clone();
    let app_state = AppState {
        manager: shared_manager,
        shutdown_tx, // Send to sender
    };
    // Keep-Alive Loop at background
    if keep_alive_seconds > 0 {
        println!(
            "🛡️ Keep-Alive system enabled. Checking every {} seconds.",
            keep_alive_seconds
        );
        // use spawn to monitor the health
        tokio::spawn(async move {
            let mut interval =
                tokio::time::interval(tokio::time::Duration::from_secs(keep_alive_seconds));

            interval.tick().await;

            loop {
                interval.tick().await;
                let mut mgr = monitor_manager.lock().await;
                let all_ids: Vec<String> = mgr.services.keys().cloned().collect();
                let mut dead_services = Vec::new();
                // find dead services
                for id in all_ids {
                    let is_running = mgr.is_running(&id);

                    if let Some(svc) = mgr.services.get(&id) {
                        if svc.config.autorun.unwrap_or(false) && !is_running {
                            dead_services.push(id);
                        }
                    }
                }
                if !dead_services.is_empty() {
                    println!(
                        "⚠️ Keep-Alive Check: Found {} stopped services. Restarting...",
                        dead_services.len()
                    );
                }
                // keep alive processing
                for id in dead_services {
                    println!("🔄 Auto-restarting service: {}", id);
                    if let Err(e) = mgr.start(&id).await {
                        eprintln!("❌ Failed to restart {}: {}", id, e);
                    }
                }
            }
        });
    }
    // create api router and listening
    let app = api::create_router(app_state).layer(CorsLayer::permissive());
    println!("🚀 Server running on http://{}", listen_addr);
    let listener = TcpListener::bind(&listen_addr).await?;
    // Decouple app manager and apps
    // The port is released when app manager exit
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawSocket;
        use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
        unsafe {
            SetHandleInformation(listener.as_raw_socket() as _, HANDLE_FLAG_INHERIT, 0);
        }
    }
    // Web frame: axum
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal(shutdown_rx))
        .await?;

    Ok(())
}
/// Process shutdown signal and exit
async fn shutdown_signal(mut api_rx: mpsc::Receiver<()>) {
    // Stop by "Ctrl+C"
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };
    // For Windows shutdown signal
    #[cfg(windows)]
    let ctrl_close = async {
        let mut stream = tokio::signal::windows::ctrl_close().unwrap();
        stream.recv().await;
    };
    // For non-Windows platform
    #[cfg(not(windows))]
    let ctrl_close = std::future::pending::<()>();
    // api stop signal
    let api_signal = async {
        api_rx.recv().await;
    };
    tokio::select! {
        _ = ctrl_c => println!("\nReceived Ctrl+C, shutting down..."),
        _ = ctrl_close => println!("\nReceived Close Event, shutting down..."),
        _ = api_signal => println!("\nReceived API Shutdown signal, shutting down..."),
    }
}
