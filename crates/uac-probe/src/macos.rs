//! Probe de macOS. Sin validar todavía en un Mac real (tarea T0.2).

use serde_json::Value;
use uac_protocol::{GpuInfo, GpuVendor, MemoryKind, NodeReport};

use crate::common::{self, Evidence};

pub fn fill(report: &mut NodeReport) {
    let sysctl = |key: &str| common::run("sysctl", &["-n", key]);

    report.os.version = common::run("sw_vers", &["-productVersion"]);
    report.cpu.model = sysctl("machdep.cpu.brand_string");
    report.cpu.physical_cores = sysctl("hw.physicalcpu").and_then(|v| v.parse().ok());
    report.cpu.logical_cores = sysctl("hw.logicalcpu").and_then(|v| v.parse().ok());
    report.ram_bytes = sysctl("hw.memsize").and_then(|v| v.parse().ok());

    match common::run("system_profiler", &["SPDisplaysDataType", "-json"]) {
        Some(json) => report.gpus = parse_displays(&json, report.ram_bytes),
        None => report
            .warnings
            .push("system_profiler no devolvió datos de GPU".to_string()),
    }
    report
        .warnings
        .push("velocidad de enlace de red no sondeada en macOS; medir con iperf3".to_string());
}

/// Interpreta `system_profiler SPDisplaysDataType -json`.
///
/// En Apple Silicon la GPU usa memoria unificada: se reporta la RAM total y el
/// planner descuenta lo que necesita el sistema.
pub fn parse_displays(json: &str, ram_bytes: Option<u64>) -> Vec<GpuInfo> {
    let Ok(root) = serde_json::from_str::<Value>(json) else {
        return Vec::new();
    };
    let Some(items) = root.get("SPDisplaysDataType").and_then(Value::as_array) else {
        return Vec::new();
    };
    items
        .iter()
        .filter_map(|item| {
            let name = item
                .get("sppci_model")
                .or_else(|| item.get("_name"))
                .and_then(Value::as_str)?
                .to_string();
            let vendor = GpuVendor::from_name(&name);
            let unified = vendor == GpuVendor::Apple;
            let ev = Evidence {
                metal: item.get("spdisplays_mtlgpufamilysupport").is_some(),
                ..Evidence::default()
            };
            Some(GpuInfo {
                backends: common::backends_for(vendor, &name, ev),
                vendor,
                vram_bytes: if unified { ram_bytes } else { None },
                memory_kind: if unified {
                    MemoryKind::Unified
                } else {
                    MemoryKind::Dedicated
                },
                driver_version: None,
                name,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use uac_protocol::{Backend, BackendStatus};

    #[test]
    fn apple_silicon_gpu_is_unified() {
        let json = r#"{"SPDisplaysDataType":[{"_name":"Apple M4","sppci_model":"Apple M4",
            "spdisplays_mtlgpufamilysupport":"spdisplays_metal4","sppci_cores":"10"}]}"#;
        let gpus = parse_displays(json, Some(24 * 1024 * 1024 * 1024));
        assert_eq!(gpus.len(), 1);
        assert_eq!(gpus[0].vendor, GpuVendor::Apple);
        assert_eq!(gpus[0].memory_kind, MemoryKind::Unified);
        assert_eq!(gpus[0].vram_bytes, Some(24 * 1024 * 1024 * 1024));
        assert_eq!(gpus[0].backends[0].backend, Backend::Metal);
        assert_eq!(gpus[0].backends[0].status, BackendStatus::Detected);
    }

    #[test]
    fn garbage_yields_nothing() {
        assert!(parse_displays("no es json", None).is_empty());
        assert!(parse_displays("{}", None).is_empty());
    }
}
