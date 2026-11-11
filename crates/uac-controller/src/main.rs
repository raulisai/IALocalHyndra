use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use uac_planner::{Device, GIB, MemoryPolicy, Placement};
use uac_protocol::{NodeReport, REPORT_SCHEMA_VERSION};

#[derive(Parser)]
#[command(
    name = "uac-controller",
    version,
    about = "Controller del Universal AI Cluster"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Tabla del clúster a partir de los JSON que genera `uac-agent probe`
    Nodes {
        /// Directorio con un JSON por nodo
        #[arg(long, default_value = "docs/hardware")]
        reports: PathBuf,
    },
    /// Decide dónde cabría una carga: un nodo, repartida o en ninguno
    Plan {
        #[arg(long, default_value = "docs/hardware")]
        reports: PathBuf,
        /// Memoria total que necesita la carga, en GiB (pesos + contexto)
        #[arg(long)]
        required_gb: f64,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().command {
        Command::Nodes { reports } => nodes(&reports),
        Command::Plan {
            reports,
            required_gb,
        } => plan(&reports, required_gb),
    }
}

fn load_reports(dir: &Path) -> anyhow::Result<Vec<NodeReport>> {
    let entries =
        std::fs::read_dir(dir).with_context(|| format!("no se pudo leer {}", dir.display()))?;
    let mut paths: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    let mut reports = Vec::new();
    for path in paths {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("no se pudo leer {}", path.display()))?;
        let report: NodeReport = serde_json::from_str(&text)
            .with_context(|| format!("{} no es un informe de nodo válido", path.display()))?;
        if report.schema_version != REPORT_SCHEMA_VERSION {
            bail!(
                "{}: esquema {} y este binario espera {}; regenerar con `uac-agent probe`",
                path.display(),
                report.schema_version,
                REPORT_SCHEMA_VERSION
            );
        }
        reports.push(report);
    }
    if reports.is_empty() {
        bail!("no hay informes .json en {}", dir.display());
    }
    Ok(reports)
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / GIB as f64
}

/// Enlace físico más rápido de un nodo, en bits por segundo.
fn best_link(report: &NodeReport) -> Option<u64> {
    report
        .nics
        .iter()
        .filter(|n| !n.is_virtual)
        .filter_map(|n| n.link_bps)
        .max()
}

fn nodes(dir: &Path) -> anyhow::Result<()> {
    let reports = load_reports(dir)?;
    let policy = MemoryPolicy::default();

    println!(
        "{:<18} {:<30} {:>9} {:>9} {:>11}  backends",
        "nodo", "gpu", "total", "usable", "enlace"
    );
    for report in &reports {
        let link = best_link(report)
            .map(|b| format!("{:.0} Mb/s", b as f64 / 1e6))
            .unwrap_or_else(|| "?".to_string());
        if report.gpus.is_empty() {
            println!(
                "{:<18} {:<30} {:>9} {:>9} {:>11}",
                report.hostname, "(sin GPU)", "-", "-", link
            );
        }
        for gpu in &report.gpus {
            let (total, usable) = match gpu.vram_bytes {
                Some(total) => (
                    format!("{:.1} GiB", gib(total)),
                    format!(
                        "{:.1} GiB",
                        gib(uac_planner::usable_bytes(total, gpu.memory_kind, &policy))
                    ),
                ),
                None => ("?".to_string(), "?".to_string()),
            };
            let backends: Vec<String> = gpu
                .backends
                .iter()
                .map(|b| format!("{:?}={:?}", b.backend, b.status).to_lowercase())
                .collect();
            println!(
                "{:<18} {:<30} {:>9} {:>9} {:>11}  {}",
                report.hostname,
                gpu.name,
                total,
                usable,
                link,
                backends.join(", ")
            );
        }
    }
    Ok(())
}

fn plan(dir: &Path, required_gb: f64) -> anyhow::Result<()> {
    if !required_gb.is_finite() || required_gb <= 0.0 {
        bail!("--required-gb debe ser un número positivo");
    }
    let reports = load_reports(dir)?;
    let devices = uac_planner::devices(&reports, &MemoryPolicy::default());
    let required = (required_gb * GIB as f64) as u64;
    let label = |d: &Device| format!("{} / {}", d.node, d.gpu);

    match uac_planner::place(required, &devices) {
        Placement::SingleNode { device } => {
            let d = &devices[device];
            println!(
                "cabe en un solo nodo, sin reparto: {} ({:.1} GiB utilizables)",
                label(d),
                gib(d.usable_bytes)
            );
        }
        Placement::Split { shares } => {
            println!("solo cabe repartido entre {} dispositivos:", shares.len());
            for share in &shares {
                let d = &devices[share.device];
                println!(
                    "  {:<50} {:>5.1} de {:.1} GiB utilizables",
                    label(d),
                    gib(share.bytes),
                    gib(d.usable_bytes)
                );
            }
            // Cota inferior del tiempo de distribuir los pesos por el enlace más lento.
            let slowest = shares
                .iter()
                .filter_map(|share| {
                    let node = &devices[share.device].node;
                    let report = reports.iter().find(|r| &r.hostname == node)?;
                    Some((share.bytes, best_link(report)?))
                })
                .min_by_key(|(_, bps)| *bps);
            if let Some((bytes, bps)) = slowest {
                if let Some(secs) = uac_planner::transfer_seconds(bytes, bps) {
                    println!(
                        "enlace más lento: {:.0} Mb/s; enviar esa parte de los pesos tarda al menos {:.0} s",
                        bps as f64 / 1e6,
                        secs
                    );
                }
            }
        }
        Placement::DoesNotFit { deficit_bytes } => {
            println!(
                "no cabe en el clúster: faltan {:.1} GiB utilizables",
                gib(deficit_bytes)
            );
        }
    }
    Ok(())
}
