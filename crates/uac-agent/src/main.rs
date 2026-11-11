use std::path::PathBuf;

use anyhow::Context;
use clap::{Parser, Subcommand};
use uac_protocol::NodeReport;

#[derive(Parser)]
#[command(
    name = "uac-agent",
    version,
    about = "Agente de nodo del Universal AI Cluster"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Inventario de hardware, SO y red de este nodo, en JSON
    Probe {
        /// Además de imprimirlo, guarda el JSON en este archivo
        #[arg(long)]
        out: Option<PathBuf>,
        /// JSON en una sola línea
        #[arg(long)]
        compact: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Probe { out, compact } => probe(out, compact),
    }
}

fn probe(out: Option<PathBuf>, compact: bool) -> anyhow::Result<()> {
    let report = uac_probe::probe();
    let json = if compact {
        serde_json::to_string(&report)?
    } else {
        serde_json::to_string_pretty(&report)?
    };
    println!("{json}");

    if let Some(path) = out {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("no se pudo crear {}", parent.display()))?;
        }
        std::fs::write(&path, format!("{json}\n"))
            .with_context(|| format!("no se pudo escribir {}", path.display()))?;
        eprintln!("guardado en {}", path.display());
    }
    summarize(&report);
    Ok(())
}

/// Resumen legible por stderr, para no ensuciar el JSON de stdout.
fn summarize(report: &NodeReport) {
    eprintln!();
    eprintln!(
        "nodo {} ({} {})",
        report.hostname, report.os.family, report.os.arch
    );
    for gpu in &report.gpus {
        let vram = gpu
            .vram_bytes
            .map(|b| format!("{:.1} GiB", b as f64 / (1u64 << 30) as f64))
            .unwrap_or_else(|| "memoria desconocida".to_string());
        let backends: Vec<String> = gpu
            .backends
            .iter()
            .map(|b| format!("{:?}={:?}", b.backend, b.status).to_lowercase())
            .collect();
        eprintln!("  gpu  {} · {vram} · {}", gpu.name, backends.join(", "));
    }
    for nic in report.nics.iter().filter(|n| !n.is_virtual) {
        let speed = nic
            .link_bps
            .map(|b| format!("{:.1} Mb/s", b as f64 / 1e6))
            .unwrap_or_else(|| "velocidad desconocida".to_string());
        eprintln!("  red  {} · {speed}", nic.name);
    }
    for warning in &report.warnings {
        eprintln!("  aviso: {warning}");
    }
}
