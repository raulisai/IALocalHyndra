//! Inventario de hardware, SO y red de un nodo.
//!
//! Los tres módulos de plataforma se compilan en todos los sistemas (solo usan
//! `std::process` y `std::fs`) y se elige uno en tiempo de ejecución. Así el
//! código de macOS y Linux se comprueba al compilar en Windows y sus parsers
//! tienen tests en cualquier máquina.
//!
//! Probado en ejecución real solo en Windows. Las rutas de macOS y Linux están
//! pendientes de validar en esos nodos (tarea T0.2).

mod common;
pub use common::no_console_window;
mod linux;
mod macos;
mod windows;

use uac_protocol::{CpuInfo, NodeReport, OsInfo, REPORT_SCHEMA_VERSION};

pub fn probe() -> NodeReport {
    let mut report = NodeReport {
        schema_version: REPORT_SCHEMA_VERSION,
        hostname: common::hostname(),
        os: OsInfo {
            family: std::env::consts::OS.to_string(),
            version: None,
            arch: std::env::consts::ARCH.to_string(),
        },
        cpu: CpuInfo::default(),
        ram_bytes: None,
        gpus: Vec::new(),
        nics: Vec::new(),
        tools: common::detect_tools(),
        warnings: Vec::new(),
    };

    match std::env::consts::OS {
        "windows" => windows::fill(&mut report),
        "macos" => macos::fill(&mut report),
        "linux" => linux::fill(&mut report),
        other => report
            .warnings
            .push(format!("sistema operativo sin probe: {other}")),
    }

    if report.gpus.is_empty() {
        report
            .warnings
            .push("no se detectó ninguna GPU".to_string());
    }
    report
}
