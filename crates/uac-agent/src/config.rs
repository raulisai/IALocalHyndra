//! Configuración local del nodo, guardada como JSON en el directorio de
//! configuración del usuario (`%APPDATA%\uac` en Windows).

use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use serde::{Deserialize, Serialize};

pub const DEFAULT_PANEL_PORT: u16 = 47800;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Nombre visible del nodo. Vacío = nombre del equipo.
    pub node_name: String,
    /// Puerto del panel web. Siempre escucha solo en 127.0.0.1.
    pub panel_port: u16,
    /// Carpeta con los binarios de llama.cpp. `None` = buscar `tools/llama.cpp/`.
    pub llama_cpp_dir: Option<PathBuf>,
    /// Carpetas donde buscar archivos `.gguf`.
    pub model_dirs: Vec<PathBuf>,
    /// Incluir los modelos descargados con Ollama.
    pub include_ollama: bool,
    pub rpc: RpcConfig,
    pub llm: LlmConfig,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RpcConfig {
    /// IP en la que escucha `ggml-rpc-server`. `127.0.0.1` = solo este equipo.
    pub bind: String,
    pub port: u16,
    /// Dispositivo que presta (p. ej. `Vulkan0`). Vacío = todos.
    pub device: String,
    /// Caché local de tensores (`-c`): evita reenviar pesos.
    pub cache: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LlmConfig {
    pub port: u16,
    pub gpu_layers: u32,
    pub ctx_size: u32,
    /// Servidores RPC de otros nodos (`ip:puerto`) entre los que repartir el modelo.
    pub rpc_peers: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        let mut model_dirs = Vec::new();
        if let Some(home) = home_dir() {
            let lmstudio = home.join(".lmstudio").join("models");
            if lmstudio.is_dir() {
                model_dirs.push(lmstudio);
            }
        }
        Self {
            node_name: String::new(),
            panel_port: DEFAULT_PANEL_PORT,
            llama_cpp_dir: None,
            model_dirs,
            include_ollama: true,
            rpc: RpcConfig::default(),
            llm: LlmConfig::default(),
        }
    }
}

impl Default for RpcConfig {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1".to_string(),
            port: 50052,
            device: String::new(),
            cache: true,
        }
    }
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            port: 8080,
            gpu_layers: 99,
            ctx_size: 4096,
            rpc_peers: Vec::new(),
        }
    }
}

impl Config {
    /// Comprueba lo que el usuario puede escribir mal en el panel.
    pub fn validate(&self) -> anyhow::Result<()> {
        for (name, port) in [
            ("panel", self.panel_port),
            ("RPC", self.rpc.port),
            ("llama-server", self.llm.port),
        ] {
            if port == 0 {
                bail!("el puerto de {name} no puede ser 0");
            }
        }
        if self.llm.port == self.panel_port || self.rpc.port == self.panel_port {
            bail!("los puertos del panel, RPC y llama-server deben ser distintos");
        }
        self.rpc
            .bind
            .parse::<IpAddr>()
            .with_context(|| format!("«{}» no es una IP válida para RPC", self.rpc.bind))?;
        if !self
            .rpc
            .device
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == ',')
        {
            bail!("el dispositivo RPC solo admite letras, números y comas (p. ej. Vulkan0)");
        }
        for peer in &self.llm.rpc_peers {
            peer.parse::<SocketAddr>()
                .with_context(|| format!("«{peer}» no tiene la forma ip:puerto"))?;
        }
        if self.llm.ctx_size < 256 {
            bail!("el contexto debe ser de al menos 256 tokens");
        }
        Ok(())
    }

    pub fn rpc_is_loopback(&self) -> bool {
        self.rpc
            .bind
            .parse::<IpAddr>()
            .map(|ip| ip.is_loopback())
            .unwrap_or(false)
    }
}

pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
}

/// Directorio de configuración y logs del programa.
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| home_dir().map(|h| h.join(".config")))
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("uac")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

/// Lee la configuración; si no existe o está dañada, usa la de por defecto.
pub fn load(path: &Path) -> (Config, Option<String>) {
    match std::fs::read_to_string(path) {
        Ok(text) => match serde_json::from_str(&text) {
            Ok(cfg) => (cfg, None),
            Err(err) => (
                Config::default(),
                Some(format!(
                    "{} no es válido ({err}); se usa la configuración por defecto",
                    path.display()
                )),
            ),
        },
        Err(_) => (Config::default(), None),
    }
}

pub fn save(path: &Path, cfg: &Config) -> anyhow::Result<()> {
    cfg.validate()?;
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("no se pudo crear {}", dir.display()))?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_string_pretty(cfg)?)
        .with_context(|| format!("no se pudo escribir {}", tmp.display()))?;
    std::fs::rename(&tmp, path)
        .with_context(|| format!("no se pudo reemplazar {}", path.display()))?;
    Ok(())
}

pub fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_string()
    }
}

fn has_llama(dir: &Path) -> bool {
    dir.join(exe_name("llama-server")).is_file()
}

/// Carpeta de llama.cpp: la configurada, o la última versión dentro de
/// `tools/llama.cpp/` buscando desde el ejecutable y el directorio actual hacia arriba.
pub fn resolve_llama_dir(cfg: &Config) -> Option<PathBuf> {
    if let Some(dir) = &cfg.llama_cpp_dir {
        return has_llama(dir).then(|| dir.clone());
    }
    let mut starts = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        starts.push(exe);
    }
    if let Ok(cwd) = std::env::current_dir() {
        starts.push(cwd);
    }
    for start in starts {
        for ancestor in start.ancestors() {
            let tools = ancestor.join("tools").join("llama.cpp");
            let Ok(entries) = std::fs::read_dir(&tools) else {
                continue;
            };
            let mut found: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| has_llama(p))
                .collect();
            found.sort();
            if let Some(last) = found.pop() {
                return Some(last);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_is_valid_and_local() {
        let cfg = Config::default();
        cfg.validate().unwrap();
        assert!(cfg.rpc_is_loopback());
    }

    #[test]
    fn rejects_bad_input() {
        let mut cfg = Config::default();
        cfg.rpc.bind = "mi-casa".to_string();
        assert!(cfg.validate().is_err());

        let mut cfg = Config::default();
        cfg.llm.rpc_peers = vec!["192.168.1.20".to_string()];
        assert!(cfg.validate().is_err());

        let mut cfg = Config::default();
        cfg.rpc.device = "Vulkan0 --host 0.0.0.0".to_string();
        assert!(cfg.validate().is_err());

        let mut cfg = Config::default();
        cfg.llm.port = cfg.panel_port;
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn lan_bind_is_detected() {
        let mut cfg = Config::default();
        cfg.rpc.bind = "192.168.1.10".to_string();
        cfg.validate().unwrap();
        assert!(!cfg.rpc_is_loopback());
    }

    #[test]
    fn missing_fields_take_defaults() {
        let cfg: Config = serde_json::from_str(r#"{"node_name":"amd"}"#).unwrap();
        assert_eq!(cfg.node_name, "amd");
        assert_eq!(cfg.panel_port, DEFAULT_PANEL_PORT);
        assert_eq!(cfg.rpc.port, 50052);
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = std::env::temp_dir().join(format!("uac-test-{}", std::process::id()));
        let path = dir.join("config.json");
        let cfg = Config {
            node_name: "prueba".to_string(),
            ..Config::default()
        };
        save(&path, &cfg).unwrap();
        let (loaded, warning) = load(&path);
        assert_eq!(loaded, cfg);
        assert!(warning.is_none());
        std::fs::remove_dir_all(&dir).ok();
    }
}
