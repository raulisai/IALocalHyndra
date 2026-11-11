//! Probe de Linux. Sin validar todavía en un nodo real (tarea T0.2).

use std::path::Path;

use uac_protocol::{GpuInfo, GpuVendor, MemoryKind, NicInfo, NodeReport};

use crate::common::{self, Evidence};

pub fn fill(report: &mut NodeReport) {
    report.os.version = common::read("/etc/os-release").and_then(|s| parse_os_release(&s));
    if let Some(cpuinfo) = common::read("/proc/cpuinfo") {
        let (model, logical) = parse_cpuinfo(&cpuinfo);
        report.cpu.model = model;
        report.cpu.logical_cores = (logical > 0).then_some(logical);
    }
    report.ram_bytes = common::read("/proc/meminfo").and_then(|s| parse_meminfo(&s));

    let vulkan = common::vulkan_loader_present();
    let nvidia = common::nvidia_smi();
    for gpu in &nvidia {
        let ev = Evidence {
            cuda: true,
            vulkan,
            metal: false,
        };
        report.gpus.push(GpuInfo {
            name: gpu.name.clone(),
            vendor: GpuVendor::Nvidia,
            vram_bytes: gpu.vram_bytes,
            memory_kind: MemoryKind::Dedicated,
            driver_version: gpu.driver.clone(),
            backends: common::backends_for(GpuVendor::Nvidia, &gpu.name, ev),
        });
    }
    report.gpus.extend(drm_gpus(vulkan, !nvidia.is_empty()));
    report.nics = nics();
}

/// GPU vistas por el kernel en `/sys/class/drm`. Las NVIDIA se omiten si
/// `nvidia-smi` ya las listó con mejor información.
fn drm_gpus(vulkan: bool, skip_nvidia: bool) -> Vec<GpuInfo> {
    let Ok(entries) = std::fs::read_dir("/sys/class/drm") else {
        return Vec::new();
    };
    let mut gpus = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let card = file_name.to_string_lossy();
        // Solo "cardN"; "cardN-HDMI-A-1" son conectores.
        let is_card = card
            .strip_prefix("card")
            .is_some_and(|rest| !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit()));
        if !is_card {
            continue;
        }
        let device = entry.path().join("device");
        let Some(vendor_id) = common::read(device.join("vendor")) else {
            continue;
        };
        let device_id = common::read(device.join("device")).unwrap_or_default();
        let (vendor, label) = match vendor_id.as_str() {
            "0x10de" => (GpuVendor::Nvidia, "NVIDIA"),
            "0x1002" => (GpuVendor::Amd, "AMD Radeon"),
            "0x8086" => (GpuVendor::Intel, "Intel"),
            _ => (GpuVendor::Other, "GPU"),
        };
        if vendor == GpuVendor::Nvidia && skip_nvidia {
            continue;
        }
        let name = format!("{label} ({vendor_id}:{device_id})");
        // amdgpu expone la VRAM total en bytes.
        let vram_bytes = common::read(device.join("mem_info_vram_total"))
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|v| *v > 0);
        let ev = Evidence {
            vulkan,
            ..Evidence::default()
        };
        gpus.push(GpuInfo {
            backends: common::backends_for(vendor, &name, ev),
            vendor,
            vram_bytes,
            memory_kind: if vendor == GpuVendor::Intel {
                MemoryKind::Shared
            } else {
                MemoryKind::Dedicated
            },
            driver_version: None,
            name,
        });
    }
    gpus
}

fn nics() -> Vec<NicInfo> {
    let Ok(entries) = std::fs::read_dir("/sys/class/net") else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().to_string();
            if name == "lo" {
                return None;
            }
            let path = entry.path();
            if common::read(path.join("operstate")).as_deref() != Some("up") {
                return None;
            }
            // `speed` está en Mb/s; da error o -1 en Wi-Fi e interfaces virtuales.
            let link_bps = common::read(path.join("speed"))
                .and_then(|s| s.parse::<i64>().ok())
                .filter(|mbps| *mbps > 0)
                .map(|mbps| mbps as u64 * 1_000_000);
            Some(NicInfo {
                is_virtual: !Path::new(&path.join("device")).exists(),
                name,
                description: None,
                link_bps,
            })
        })
        .collect()
}

/// `MemTotal` de `/proc/meminfo`, en bytes.
pub fn parse_meminfo(text: &str) -> Option<u64> {
    let line = text.lines().find(|l| l.starts_with("MemTotal:"))?;
    let kb: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

/// Modelo de CPU y número de procesadores lógicos de `/proc/cpuinfo`.
pub fn parse_cpuinfo(text: &str) -> (Option<String>, u32) {
    let model = text
        .lines()
        .find(|l| l.starts_with("model name"))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string());
    let logical = text.lines().filter(|l| l.starts_with("processor")).count() as u32;
    (model, logical)
}

pub fn parse_os_release(text: &str) -> Option<String> {
    text.lines()
        .find_map(|l| l.strip_prefix("PRETTY_NAME="))
        .map(|v| v.trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn meminfo() {
        let text = "MemTotal:       32790148 kB\nMemFree:        1000 kB\n";
        assert_eq!(parse_meminfo(text), Some(32790148 * 1024));
        assert_eq!(parse_meminfo("MemFree: 1 kB"), None);
    }

    #[test]
    fn cpuinfo() {
        let text = "processor\t: 0\nmodel name\t: AMD Ryzen 5 3400G\nprocessor\t: 1\nmodel name\t: AMD Ryzen 5 3400G\n";
        assert_eq!(
            parse_cpuinfo(text),
            (Some("AMD Ryzen 5 3400G".to_string()), 2)
        );
    }

    #[test]
    fn os_release() {
        let text = "NAME=\"Ubuntu\"\nPRETTY_NAME=\"Ubuntu 24.04.4 LTS\"\n";
        assert_eq!(
            parse_os_release(text),
            Some("Ubuntu 24.04.4 LTS".to_string())
        );
    }
}
