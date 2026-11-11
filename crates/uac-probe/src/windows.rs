use serde::de::DeserializeOwned;
use serde::{Deserialize, Deserializer};
use serde_json::Value;
use uac_protocol::{GpuInfo, GpuVendor, MemoryKind, NicInfo, NodeReport};

use crate::common::{self, Evidence};

/// Una sola invocación de PowerShell que devuelve todo en JSON.
///
/// La VRAM sale del registro (`HardwareInformation.qwMemorySize`, 64 bits)
/// porque `Win32_VideoController.AdapterRAM` es de 32 bits y se satura en 4 GB.
/// El registro conserva adaptadores que ya no están activos, así que la lista
/// de GPU se toma de `Win32_VideoController` y el registro solo aporta la VRAM.
const SCRIPT: &str = r#"
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$os = Get-CimInstance Win32_OperatingSystem
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$cs = Get-CimInstance Win32_ComputerSystem
$reg = @{}
Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}\0*' | ForEach-Object {
    if ($_.DriverDesc) {
        $q = $_.'HardwareInformation.qwMemorySize'
        $v = [uint64]0
        if ($q -is [array]) { $v = [BitConverter]::ToUInt64([byte[]]$q, 0) } elseif ($q) { $v = [uint64]$q }
        if ($v -gt 0) { $reg[$_.DriverDesc] = $v }
    }
}
$gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
    $src = 'registry'
    $v = $reg[$_.Name]
    if (-not $v) { $v = [uint64]$_.AdapterRAM; $src = 'adapter_ram' }
    [pscustomobject]@{ name = $_.Name; vram = [uint64]$v; vram_source = $src; driver = $_.DriverVersion }
})
$nics = @(Get-NetAdapter | Where-Object Status -eq 'Up' | ForEach-Object {
    [pscustomobject]@{ name = $_.Name; description = $_.InterfaceDescription; bps = [uint64]$_.Speed; virtual = [bool]$_.Virtual }
})
[pscustomobject]@{
    os_name = $os.Caption; os_version = $os.Version
    cpu = $cpu.Name; cores = $cpu.NumberOfCores; threads = $cpu.NumberOfLogicalProcessors
    ram = [uint64]$cs.TotalPhysicalMemory
    gpus = $gpus; nics = $nics
} | ConvertTo-Json -Depth 4 -Compress
"#;

#[derive(Debug, Deserialize)]
struct Raw {
    os_name: Option<String>,
    os_version: Option<String>,
    cpu: Option<String>,
    cores: Option<u32>,
    threads: Option<u32>,
    ram: Option<u64>,
    #[serde(default, deserialize_with = "one_or_many")]
    gpus: Vec<RawGpu>,
    #[serde(default, deserialize_with = "one_or_many")]
    nics: Vec<RawNic>,
}

#[derive(Debug, Deserialize)]
struct RawGpu {
    name: String,
    vram: Option<u64>,
    vram_source: Option<String>,
    driver: Option<String>,
}

#[derive(Debug, Deserialize)]
struct RawNic {
    name: String,
    description: Option<String>,
    bps: Option<u64>,
    #[serde(default, rename = "virtual")]
    is_virtual: bool,
}

/// Windows PowerShell 5.1 serializa las colecciones de forma irregular: un
/// array, un objeto suelto, `null` o `{"value": [...], "Count": n}`.
fn one_or_many<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let items = match Value::deserialize(deserializer)? {
        Value::Null => Vec::new(),
        Value::Array(items) => items,
        Value::Object(mut map) if map.get("value").is_some_and(Value::is_array) => {
            match map.remove("value") {
                Some(Value::Array(items)) => items,
                _ => Vec::new(),
            }
        }
        single => vec![single],
    };
    items
        .into_iter()
        .map(|item| serde_json::from_value(item).map_err(serde::de::Error::custom))
        .collect()
}

pub fn fill(report: &mut NodeReport) {
    let Some(json) = common::run(
        "powershell",
        &["-NoProfile", "-NonInteractive", "-Command", SCRIPT],
    ) else {
        report
            .warnings
            .push("PowerShell no devolvió datos de hardware".to_string());
        return;
    };
    let ev = Evidence {
        vulkan: common::vulkan_loader_present(),
        ..Evidence::default()
    };
    if let Err(err) = apply(report, &json, &common::nvidia_smi(), ev) {
        report.warnings.push(format!(
            "no se pudo interpretar la salida de PowerShell: {err}"
        ));
    }
}

fn apply(
    report: &mut NodeReport,
    json: &str,
    nvidia: &[common::NvidiaGpu],
    ev: Evidence,
) -> Result<(), serde_json::Error> {
    let raw: Raw = serde_json::from_str(json)?;

    report.os.version = match (raw.os_name, raw.os_version) {
        (Some(name), Some(version)) => Some(format!("{} ({version})", name.trim())),
        (name, version) => name.or(version),
    };
    report.cpu.model = raw.cpu.map(|c| c.trim().to_string());
    report.cpu.physical_cores = raw.cores;
    report.cpu.logical_cores = raw.threads;
    report.ram_bytes = raw.ram.filter(|r| *r > 0);

    for gpu in raw.gpus {
        let vendor = GpuVendor::from_name(&gpu.name);
        let smi = nvidia.iter().find(|n| n.name == gpu.name);
        let mut vram = gpu.vram.filter(|v| *v > 0);
        if let Some(bytes) = smi.and_then(|n| n.vram_bytes) {
            vram = Some(bytes);
        } else if gpu.vram_source.as_deref() == Some("adapter_ram") {
            report.warnings.push(format!(
                "{}: VRAM tomada de AdapterRAM, que se satura en 4 GB; confirmar a mano",
                gpu.name
            ));
        }
        let memory_kind = if common::is_integrated(&gpu.name) {
            MemoryKind::Shared
        } else {
            MemoryKind::Dedicated
        };
        let backends = common::backends_for(
            vendor,
            &gpu.name,
            Evidence {
                cuda: smi.is_some(),
                ..ev
            },
        );
        report.gpus.push(GpuInfo {
            name: gpu.name,
            vendor,
            vram_bytes: vram,
            memory_kind,
            driver_version: smi.and_then(|n| n.driver.clone()).or(gpu.driver),
            backends,
        });
    }

    report.nics = raw
        .nics
        .into_iter()
        .map(|nic| NicInfo {
            name: nic.name,
            description: nic.description,
            link_bps: nic.bps.filter(|b| *b > 0),
            is_virtual: nic.is_virtual,
        })
        .collect();
    if !report.nics.iter().any(|n| !n.is_virtual) {
        report
            .warnings
            .push("no hay ninguna interfaz de red física activa".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use uac_protocol::{Backend, BackendStatus, CpuInfo, OsInfo, REPORT_SCHEMA_VERSION};

    fn empty_report() -> NodeReport {
        NodeReport {
            schema_version: REPORT_SCHEMA_VERSION,
            hostname: "test".to_string(),
            os: OsInfo::default(),
            cpu: CpuInfo::default(),
            ram_bytes: None,
            gpus: Vec::new(),
            nics: Vec::new(),
            tools: Vec::new(),
            warnings: Vec::new(),
        }
    }

    #[test]
    fn parses_arrays() {
        let json = r#"{"os_name":"Microsoft Windows 11 Pro","os_version":"10.0.26300",
            "cpu":"AMD Ryzen 5 3400G ","cores":4,"threads":8,"ram":34000000000,
            "gpus":[{"name":"AMD Radeon RX 6600","vram":8573157376,"vram_source":"registry","driver":"32.0"}],
            "nics":[{"name":"Wi-Fi","description":"Intel AX200","bps":292500000,"virtual":false},
                    {"name":"vEthernet","description":"Hyper-V","bps":10000000000,"virtual":true}]}"#;
        let mut report = empty_report();
        let ev = Evidence {
            vulkan: true,
            ..Evidence::default()
        };
        apply(&mut report, json, &[], ev).unwrap();

        assert_eq!(report.cpu.model.as_deref(), Some("AMD Ryzen 5 3400G"));
        assert_eq!(report.gpus.len(), 1);
        let gpu = &report.gpus[0];
        assert_eq!(gpu.vram_bytes, Some(8573157376));
        assert_eq!(gpu.memory_kind, MemoryKind::Dedicated);
        assert!(
            gpu.backends
                .iter()
                .any(|b| b.backend == Backend::Vulkan && b.status == BackendStatus::Detected)
        );
        assert_eq!(report.nics.len(), 2);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn parses_single_object_and_wrapped_collections() {
        let json = r#"{"os_name":null,"os_version":"10.0","cpu":null,"cores":null,"threads":null,"ram":0,
            "gpus":{"name":"NVIDIA GeForce RTX 3070","vram":4293918720,"vram_source":"adapter_ram","driver":"1"},
            "nics":{"value":[{"name":"Ethernet","description":null,"bps":1000000000,"virtual":false}],"Count":1}}"#;
        let nvidia = [common::NvidiaGpu {
            name: "NVIDIA GeForce RTX 3070".to_string(),
            vram_bytes: Some(8192 * 1024 * 1024),
            driver: Some("551.23".to_string()),
        }];
        let mut report = empty_report();
        apply(&mut report, json, &nvidia, Evidence::default()).unwrap();

        assert_eq!(report.ram_bytes, None);
        let gpu = &report.gpus[0];
        // nvidia-smi manda sobre el valor saturado de AdapterRAM.
        assert_eq!(gpu.vram_bytes, Some(8192 * 1024 * 1024));
        assert_eq!(gpu.driver_version.as_deref(), Some("551.23"));
        assert_eq!(gpu.backends[0].status, BackendStatus::Detected);
        assert_eq!(report.nics.len(), 1);
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn warns_when_vram_comes_from_adapter_ram() {
        let json = r#"{"gpus":[{"name":"AMD Radeon RX 6600","vram":4293918720,"vram_source":"adapter_ram","driver":null}],"nics":null}"#;
        let mut report = empty_report();
        apply(&mut report, json, &[], Evidence::default()).unwrap();
        assert!(report.warnings.iter().any(|w| w.contains("AdapterRAM")));
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.contains("interfaz de red"))
        );
    }
}
