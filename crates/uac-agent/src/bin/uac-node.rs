// En release no abre ventana de consola: solo el icono de la bandeja.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

use uac_agent::{Options, log_line, run_node};

fn main() {
    let open_browser = !std::env::args().any(|a| a == "--no-browser");
    if let Err(err) = run_node(Options { open_browser }) {
        log_line(&format!("error: {err:#}"));
        std::process::exit(1);
    }
}
