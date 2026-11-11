use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use uac_protocol::{Backend, BackendStatus, BackendSupport, GpuVendor, ToolInfo};

/// Herramientas cuya presencia interesa para las fases 0 a 2.
const TOOLS: &[&str] = &[
    "git",
    "cargo",
    "python",
    "python3",
    "uv",
    "node",
    "cmake",
    "nvidia-smi",
    "vulkaninfo",
    "iperf3",
    "llama-server",
    "llama-bench",
    "ggml-rpc-server",
];

/// Ejecuta un comando y devuelve su salida estándar recortada si terminó bien.
pub fn run(cmd: &str, args: &[&str]) -> Option<String> {
    let out = Command::new(cmd)
        .args(args)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!text.is_empty()).then_some(text)
}

pub fn read(path: impl AsRef<Path>) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_string())
}

pub fn hostname() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .or_else(|| std::env::var("HOSTNAME").ok())
        .or_else(|| run("hostname", &[]))
        .unwrap_or_else(|| "desconocido".to_string())
}

pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let exts: &[&str] = if cfg!(windows) {
        &[".exe", ".cmd", ".bat"]
    } else {
        &[""]
    };
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path)
        .flat_map(|dir| exts.iter().map(move |ext| dir.join(format!("{name}{ext}"))))
        .find(|candidate| candidate.is_file())
}

pub fn detect_tools() -> Vec<ToolInfo> {
    TOOLS
        .iter()
        .map(|name| ToolInfo {
            name: (*name).to_string(),
            path: find_on_path(name).map(|p| p.display().to_string()),
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NvidiaGpu {
    pub name: String,
    pub vram_bytes: Option<u64>,
    pub driver: Option<String>,
}

/// Parsea `nvidia-smi --query-gpu=name,memory.total,driver_version
/// --format=csv,noheader,nounits` (memoria en MiB).
pub fn parse_nvidia_smi(csv: &str) -> Vec<NvidiaGpu> {
    csv.lines()
        .filter_map(|line| {
            let mut cols = line.split(',').map(str::trim);
            let name = cols.next().filter(|n| !n.is_empty())?;
            let vram_bytes = cols
                .next()
                .and_then(|m| m.parse::<u64>().ok())
                .map(|mib| mib * 1024 * 1024);
            let driver = cols.next().filter(|d| !d.is_empty()).map(str::to_string);
            Some(NvidiaGpu {
                name: name.to_string(),
                vram_bytes,
                driver,
            })
        })
        .collect()
}

pub fn nvidia_smi() -> Vec<NvidiaGpu> {
    run(
        "nvidia-smi",
        &[
            "--query-gpu=name,memory.total,driver_version",
            "--format=csv,noheader,nounits",
        ],
    )
    .map(|csv| parse_nvidia_smi(&csv))
    .unwrap_or_default()
}

pub fn vulkan_loader_present() -> bool {
    match std::env::consts::OS {
        "windows" => std::env::var_os("WINDIR")
            .map(|w| {
                PathBuf::from(w)
                    .join("System32")
                    .join("vulkan-1.dll")
                    .is_file()
            })
            .unwrap_or(false),
        "linux" => [
            "/usr/lib/x86_64-linux-gnu/libvulkan.so.1",
            "/usr/lib/aarch64-linux-gnu/libvulkan.so.1",
            "/usr/lib64/libvulkan.so.1",
            "/usr/lib/libvulkan.so.1",
        ]
        .iter()
        .any(|p| Path::new(p).exists()),
        _ => false,
    }
}

/// Heurística por nombre para GPU integradas que toman RAM del sistema.
pub fn is_integrated(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "vega",
        "uhd graphics",
        "iris",
        "radeon(tm) graphics",
        "radeon graphics",
    ]
    .iter()
    .any(|pat| n.contains(pat))
}

/// Evidencia local recogida por el probe.
#[derive(Debug, Clone, Copy, Default)]
pub struct Evidence {
    /// `nvidia-smi` respondió y listó esta GPU.
    pub cuda: bool,
    /// El cargador de Vulkan está instalado.
    pub vulkan: bool,
    /// El sistema reporta soporte Metal para esta GPU.
    pub metal: bool,
}

/// Backends candidatos de una GPU con el grado de evidencia de cada uno.
/// Nunca devuelve `Verified`: eso requiere un benchmark (T0.6).
pub fn backends_for(vendor: GpuVendor, name: &str, ev: Evidence) -> Vec<BackendSupport> {
    let vulkan = BackendSupport {
        backend: Backend::Vulkan,
        status: if ev.vulkan {
            BackendStatus::Detected
        } else {
            BackendStatus::Unknown
        },
        note: None,
    };
    match vendor {
        GpuVendor::Nvidia => vec![
            BackendSupport {
                backend: Backend::Cuda,
                status: if ev.cuda {
                    BackendStatus::Detected
                } else {
                    BackendStatus::Expected
                },
                note: (!ev.cuda).then(|| "nvidia-smi no respondió; revisar el driver".to_string()),
            },
            vulkan,
        ],
        GpuVendor::Amd => vec![vulkan, rocm_support(name)],
        GpuVendor::Apple => vec![BackendSupport {
            backend: Backend::Metal,
            status: if ev.metal {
                BackendStatus::Detected
            } else {
                BackendStatus::Expected
            },
            note: None,
        }],
        GpuVendor::Intel | GpuVendor::Other => vec![vulkan],
    }
}

/// ROCm según las matrices de compatibilidad de AMD (7.2.1 para Radeon y 10.1.0
/// unificada, consultadas el 2026-11-11): de consumo, solo RX 7600+ y RX 9000.
fn rocm_support(name: &str) -> BackendSupport {
    let n = name.to_ascii_lowercase();
    let older = ["rx 6", "rx 5", "rx 4", "vega"];
    if older.iter().any(|pat| n.contains(pat)) || is_integrated(name) {
        BackendSupport {
            backend: Backend::Rocm,
            status: BackendStatus::Unsupported,
            note: Some("serie ausente de las matrices ROCm de AMD (7.2.1 y 10.1.0)".to_string()),
        }
    } else {
        BackendSupport {
            backend: Backend::Rocm,
            status: BackendStatus::Unknown,
            note: Some("comprobar el modelo exacto en la matriz ROCm de AMD".to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvidia_smi_csv() {
        let gpus = parse_nvidia_smi("NVIDIA GeForce RTX 3070, 8192, 551.23\n");
        assert_eq!(
            gpus,
            vec![NvidiaGpu {
                name: "NVIDIA GeForce RTX 3070".to_string(),
                vram_bytes: Some(8192 * 1024 * 1024),
                driver: Some("551.23".to_string()),
            }]
        );
        assert!(parse_nvidia_smi("").is_empty());
    }

    #[test]
    fn rx6600_is_vulkan_without_rocm() {
        let ev = Evidence {
            vulkan: true,
            ..Evidence::default()
        };
        let backends = backends_for(GpuVendor::Amd, "AMD Radeon RX 6600", ev);
        let status = |b: Backend| backends.iter().find(|s| s.backend == b).map(|s| s.status);
        assert_eq!(status(Backend::Vulkan), Some(BackendStatus::Detected));
        assert_eq!(status(Backend::Rocm), Some(BackendStatus::Unsupported));
        assert_eq!(status(Backend::Cuda), None);
    }

    #[test]
    fn newer_radeon_is_not_assumed() {
        let backends = backends_for(GpuVendor::Amd, "AMD Radeon RX 9070 XT", Evidence::default());
        let rocm = backends
            .iter()
            .find(|s| s.backend == Backend::Rocm)
            .unwrap();
        assert_eq!(rocm.status, BackendStatus::Unknown);
    }

    #[test]
    fn nvidia_without_driver_is_only_expected() {
        let backends = backends_for(
            GpuVendor::Nvidia,
            "NVIDIA GeForce RTX 3070",
            Evidence::default(),
        );
        assert_eq!(backends[0].backend, Backend::Cuda);
        assert_eq!(backends[0].status, BackendStatus::Expected);
        assert!(backends[0].note.is_some());
    }

    #[test]
    fn integrated_names() {
        assert!(is_integrated("AMD Radeon(TM) RX Vega 11 Graphics"));
        assert!(is_integrated("Intel(R) UHD Graphics 770"));
        assert!(!is_integrated("AMD Radeon RX 6600"));
        assert!(!is_integrated("NVIDIA GeForce RTX 3070"));
    }
}
