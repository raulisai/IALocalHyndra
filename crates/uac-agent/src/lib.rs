//! Agente de nodo: panel web local, icono de bandeja y procesos de llama.cpp.

pub mod config;
pub mod models;
pub mod procs;
#[cfg(windows)]
pub mod tray;
pub mod web;

use std::io::Write;
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use anyhow::Context;

use crate::procs::Procs;
use crate::web::AppState;

/// Abre una URL en el navegador por defecto.
pub fn open_browser(url: &str) {
    let mut command = if cfg!(windows) {
        let mut c = std::process::Command::new("rundll32");
        c.args(["url.dll,FileProtocolHandler", url]);
        c
    } else if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("open");
        c.arg(url);
        c
    } else {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(url);
        c
    };
    if let Err(err) = command.spawn() {
        log_line(&format!("no se pudo abrir el navegador: {err}"));
    }
}

/// Añade una línea a `%APPDATA%\uac\uac-node.log`. Es la única salida visible
/// cuando el programa corre sin consola.
pub fn log_line(msg: &str) {
    eprintln!("{msg}");
    let dir = config::config_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("uac-node.log"))
    {
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "[{secs}] {msg}");
    }
}

pub struct Options {
    pub open_browser: bool,
}

/// Arranca el nodo: panel web en un hilo aparte y, en Windows, el icono de
/// bandeja en el hilo principal. Si ya hay otra instancia, abre su panel y sale.
pub fn run_node(opts: Options) -> anyhow::Result<()> {
    let config_path = config::config_path();
    let (cfg, config_warning) = config::load(&config_path);
    if let Some(w) = &config_warning {
        log_line(w);
    }

    let addr = SocketAddr::from(([127, 0, 0, 1], cfg.panel_port));
    let url = format!("http://{addr}/");
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(err) if err.kind() == std::io::ErrorKind::AddrInUse => {
            log_line(&format!("ya hay un nodo en {url}; se abre su panel"));
            open_browser(&url);
            return Ok(());
        }
        Err(err) => return Err(err).with_context(|| format!("no se pudo escuchar en {addr}")),
    };
    listener.set_nonblocking(true)?;

    let state = Arc::new(AppState {
        config: RwLock::new(cfg),
        config_path,
        config_warning,
        report: RwLock::new(None),
        procs: Procs::default(),
        started: Instant::now(),
        panel_url: url.clone(),
    });

    // El sondeo tarda un par de segundos: se hace en segundo plano y el panel
    // muestra «sondeando…» mientras tanto.
    {
        let state = state.clone();
        std::thread::spawn(move || {
            let report = uac_probe::probe();
            if let Ok(mut slot) = state.report.write() {
                *slot = Some(report);
            }
        });
    }

    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
    let server_state = state.clone();
    let server = std::thread::spawn(move || -> anyhow::Result<()> {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        rt.block_on(async move {
            let listener = tokio::net::TcpListener::from_std(listener)?;
            axum::serve(listener, web::router(server_state))
                .with_graceful_shutdown(async {
                    let _ = shutdown_rx.await;
                })
                .await?;
            anyhow::Ok(())
        })
    });

    log_line(&format!("nodo activo en {url}"));
    if opts.open_browser {
        open_browser(&url);
    }

    #[cfg(windows)]
    {
        let tooltip = format!("Universal AI Cluster: activo\n{url}");
        tray::run(url, tooltip, move || {
            state.procs.stop_all();
            let _ = shutdown_tx.send(());
            let _ = server.join();
            log_line("nodo detenido");
        });
    }

    #[cfg(not(windows))]
    {
        // Sin icono de bandeja fuera de Windows por ahora: el proceso vive
        // mientras viva el servidor (Ctrl+C para salir).
        let _ = shutdown_tx;
        let result = server
            .join()
            .map_err(|_| anyhow::anyhow!("el servidor terminó con pánico"))?;
        state.procs.stop_all();
        result
    }
}
