//! Búsqueda de modelos GGUF locales: carpetas configuradas y biblioteca de Ollama.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

use crate::config::{Config, home_dir};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Model {
    pub name: String,
    pub path: PathBuf,
    pub size_bytes: u64,
    /// `ollama` o `carpeta`.
    pub source: &'static str,
}

const MAX_DEPTH: usize = 6;

pub fn scan(cfg: &Config) -> Vec<Model> {
    let mut models = Vec::new();
    for dir in &cfg.model_dirs {
        walk(dir, dir, 0, &mut models);
    }
    if cfg.include_ollama {
        if let Some(home) = home_dir() {
            models.extend(scan_ollama(&home.join(".ollama").join("models")));
        }
    }
    models.sort_by(|a, b| a.name.cmp(&b.name));
    models.dedup_by(|a, b| a.path == b.path);
    models
}

fn walk(root: &Path, dir: &Path, depth: usize, out: &mut Vec<Model>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            walk(root, &path, depth + 1, out);
        } else if is_model_file(&path) {
            let name = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push(Model {
                name,
                path,
                size_bytes: meta.len(),
                source: "carpeta",
            });
        }
    }
}

/// Un `.gguf` que es un modelo de lenguaje. Los proyectores multimodales
/// (`mmproj-*`) no se pueden cargar solos.
fn is_model_file(path: &Path) -> bool {
    let is_gguf = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("gguf"));
    let is_projector = path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n.to_ascii_lowercase().starts_with("mmproj"));
    is_gguf && !is_projector
}

/// Modelos de Ollama: cada manifiesto apunta por hash a un blob que es un GGUF.
pub fn scan_ollama(models_dir: &Path) -> Vec<Model> {
    let manifests = models_dir.join("manifests");
    let mut files = Vec::new();
    collect_files(&manifests, 0, &mut files);
    files
        .into_iter()
        .filter_map(|manifest| {
            let text = std::fs::read_to_string(&manifest).ok()?;
            let digest = model_digest(&text)?;
            let blob = models_dir.join("blobs").join(digest.replace(':', "-"));
            let size_bytes = std::fs::metadata(&blob).ok()?.len();
            Some(Model {
                name: ollama_name(&manifests, &manifest)?,
                path: blob,
                size_bytes,
                source: "ollama",
            })
        })
        .collect()
}

fn collect_files(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_files(&path, depth + 1, out);
        } else {
            out.push(path);
        }
    }
}

/// Hash de la capa de pesos de un manifiesto de Ollama.
pub fn model_digest(manifest_json: &str) -> Option<String> {
    let root: Value = serde_json::from_str(manifest_json).ok()?;
    root.get("layers")?
        .as_array()?
        .iter()
        .find(|layer| {
            layer
                .get("mediaType")
                .and_then(Value::as_str)
                .is_some_and(|m| m == "application/vnd.ollama.image.model")
        })?
        .get("digest")?
        .as_str()
        .filter(|d| d.starts_with("sha256:") && d[7..].chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
}

/// `manifests/registry.ollama.ai/library/qwen2.5/7b` → `qwen2.5:7b`.
fn ollama_name(manifests: &Path, manifest: &Path) -> Option<String> {
    let rel = manifest.strip_prefix(manifests).ok()?;
    let parts: Vec<String> = rel
        .components()
        .map(|c| c.as_os_str().to_string_lossy().to_string())
        .collect();
    let [.., model, tag] = parts.as_slice() else {
        return None;
    };
    Some(format!("{model}:{tag}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_files() {
        assert!(is_model_file(Path::new("a/Qwen3-27B-Q4_K_M.gguf")));
        assert!(is_model_file(Path::new("a/X.GGUF")));
        assert!(!is_model_file(Path::new("a/mmproj-Qwen3-BF16.gguf")));
        assert!(!is_model_file(Path::new("a/modelo.safetensors")));
    }

    #[test]
    fn digest_from_manifest() {
        let json = r#"{"layers":[
            {"mediaType":"application/vnd.ollama.image.template","digest":"sha256:aa"},
            {"mediaType":"application/vnd.ollama.image.model","digest":"sha256:2bada8a7"}]}"#;
        assert_eq!(model_digest(json).as_deref(), Some("sha256:2bada8a7"));
        assert_eq!(model_digest(r#"{"layers":[]}"#), None);
        // Un digest con separadores de ruta no debe aceptarse.
        let evil = r#"{"layers":[{"mediaType":"application/vnd.ollama.image.model","digest":"sha256:../../x"}]}"#;
        assert_eq!(model_digest(evil), None);
    }

    #[test]
    fn names_from_manifest_path() {
        let root = Path::new("m");
        let path = Path::new("m/registry.ollama.ai/library/qwen2.5/7b");
        assert_eq!(ollama_name(root, path).as_deref(), Some("qwen2.5:7b"));
    }
}
