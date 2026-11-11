//! Tipos compartidos entre agente, controller y planner.
//!
//! Por ahora solo contiene el informe de hardware de un nodo. Los mensajes de
//! control (Register, Heartbeat, Lease, JobAssign...) llegan con la tarea T1.2.

use serde::{Deserialize, Serialize};

/// Versión del esquema de [`NodeReport`]. Subirla ante cualquier cambio incompatible.
pub const REPORT_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Apple,
    Intel,
    Other,
}

impl GpuVendor {
    pub fn from_name(name: &str) -> Self {
        let n = name.to_ascii_lowercase();
        if n.contains("nvidia") || n.contains("geforce") || n.contains("quadro") {
            Self::Nvidia
        } else if n.contains("amd") || n.contains("radeon") {
            Self::Amd
        } else if n.contains("apple") {
            Self::Apple
        } else if n.contains("intel") {
            Self::Intel
        } else {
            Self::Other
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    /// VRAM propia de una GPU discreta.
    Dedicated,
    /// Memoria unificada (Apple Silicon): se comparte con el sistema.
    Unified,
    /// GPU integrada que toma RAM del sistema. El planner no la usa.
    Shared,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Cuda,
    Metal,
    Vulkan,
    Rocm,
}

/// Grado de evidencia de que un backend sirve en una GPU.
///
/// El probe nunca emite `Verified`: ese estado lo asigna un benchmark que haya
/// calculado tensores de verdad (tarea T0.6).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackendStatus {
    /// Un benchmark ejecutó cómputo correcto en este backend.
    Verified,
    /// Hay evidencia en tiempo de ejecución (driver o cargador presentes).
    Detected,
    /// Se infiere por fabricante, sin evidencia local.
    Expected,
    /// Se sabe que no hay soporte oficial.
    Unsupported,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackendSupport {
    pub backend: Backend,
    pub status: BackendStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuInfo {
    pub name: String,
    pub vendor: GpuVendor,
    /// Memoria total. En `Unified` es la RAM completa del equipo.
    pub vram_bytes: Option<u64>,
    pub memory_kind: MemoryKind,
    pub driver_version: Option<String>,
    pub backends: Vec<BackendSupport>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NicInfo {
    pub name: String,
    pub description: Option<String>,
    /// Velocidad de enlace negociada, no ancho de banda medido.
    pub link_bps: Option<u64>,
    pub is_virtual: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct OsInfo {
    pub family: String,
    pub version: Option<String>,
    pub arch: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuInfo {
    pub model: Option<String>,
    pub physical_cores: Option<u32>,
    pub logical_cores: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolInfo {
    pub name: String,
    pub path: Option<String>,
}

/// Inventario de un nodo. Es un inventario, no una prueba de cómputo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NodeReport {
    pub schema_version: u32,
    pub hostname: String,
    pub os: OsInfo,
    pub cpu: CpuInfo,
    pub ram_bytes: Option<u64>,
    pub gpus: Vec<GpuInfo>,
    pub nics: Vec<NicInfo>,
    pub tools: Vec<ToolInfo>,
    /// Cosas que el probe no pudo determinar o que conviene revisar a mano.
    pub warnings: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vendor_from_name() {
        assert_eq!(GpuVendor::from_name("AMD Radeon RX 6600"), GpuVendor::Amd);
        assert_eq!(
            GpuVendor::from_name("NVIDIA GeForce RTX 3070"),
            GpuVendor::Nvidia
        );
        assert_eq!(GpuVendor::from_name("Apple M4"), GpuVendor::Apple);
        assert_eq!(
            GpuVendor::from_name("Intel(R) UHD Graphics 770"),
            GpuVendor::Intel
        );
        assert_eq!(
            GpuVendor::from_name("Microsoft Basic Display Adapter"),
            GpuVendor::Other
        );
    }
}
