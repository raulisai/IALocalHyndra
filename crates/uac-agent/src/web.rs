//! Panel web local. Escucha solo en 127.0.0.1.
//!
//! Defensas para una API local que puede arrancar procesos:
//! - la cabecera `Host` debe ser 127.0.0.1 o localhost (evita DNS rebinding);
//! - toda petición que modifica algo exige la cabecera `X-UAC-Panel`. Un sitio
//!   web ajeno no puede enviarla sin un preflight CORS que este servidor no
//!   acepta, así que no puede disparar acciones desde el navegador.

use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::{Value, json};
use uac_protocol::NodeReport;

use crate::config::{self, Config};
use crate::models::{self, Model};
use crate::procs::{Kind, Procs};

const INDEX_HTML: &str = include_str!("../static/index.html");
pub const ACTION_HEADER: &str = "x-uac-panel";

pub struct AppState {
    pub config: RwLock<Config>,
    pub config_path: PathBuf,
    pub config_warning: Option<String>,
    pub report: RwLock<Option<NodeReport>>,
    pub procs: Procs,
    pub started: Instant,
    pub panel_url: String,
}

pub type Shared = Arc<AppState>;

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/models", get(list_models))
        .route("/api/probe", post(reprobe))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/rpc/start", post(rpc_start))
        .route("/api/rpc/stop", post(rpc_stop))
        .route("/api/llm/start", post(llm_start))
        .route("/api/llm/stop", post(llm_stop))
        .layer(middleware::from_fn(guard))
        .with_state(state)
}

async fn guard(req: Request, next: Next) -> Response {
    let host_ok = req
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .map(is_local_host)
        .unwrap_or(false);
    if !host_ok {
        return (StatusCode::FORBIDDEN, "host no permitido").into_response();
    }
    let mutating = req.method() != Method::GET && req.method() != Method::HEAD;
    if mutating && !req.headers().contains_key(ACTION_HEADER) {
        return (StatusCode::FORBIDDEN, "falta la cabecera del panel").into_response();
    }
    next.run(req).await
}

pub fn is_local_host(host: &str) -> bool {
    let name = host.rsplit_once(':').map(|(h, _)| h).unwrap_or(host);
    matches!(name, "127.0.0.1" | "localhost" | "[::1]")
}

struct ApiError(StatusCode, String);

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.0, Json(json!({ "error": self.1 }))).into_response()
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(err: anyhow::Error) -> Self {
        ApiError(StatusCode::BAD_REQUEST, format!("{err:#}"))
    }
}

fn internal(msg: &str) -> ApiError {
    ApiError(StatusCode::INTERNAL_SERVER_ERROR, msg.to_string())
}

fn read_config(state: &AppState) -> Result<Config, ApiError> {
    state
        .config
        .read()
        .map(|c| c.clone())
        .map_err(|_| internal("configuración bloqueada"))
}

async fn index() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Html(INDEX_HTML))
}

async fn status(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    let cfg = read_config(&state)?;
    let report = state
        .report
        .read()
        .map_err(|_| internal("informe bloqueado"))?
        .clone();
    let llama_dir = config::resolve_llama_dir(&cfg);
    let tools = llama_dir.as_ref().map(|dir| {
        ["llama-server", "ggml-rpc-server", "llama-bench"]
            .iter()
            .map(|name| (name.to_string(), dir.join(config::exe_name(name)).is_file()))
            .collect::<std::collections::BTreeMap<_, _>>()
    });
    let node_name = if cfg.node_name.is_empty() {
        report
            .as_ref()
            .map(|r| r.hostname.clone())
            .unwrap_or_default()
    } else {
        cfg.node_name.clone()
    };
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "node_name": node_name,
        "uptime_secs": state.started.elapsed().as_secs(),
        "panel_url": state.panel_url,
        "config_path": state.config_path,
        "config_warning": state.config_warning,
        "config": cfg,
        "llama": { "dir": llama_dir, "tools": tools },
        "report": report,
        "rpc": state.procs.status(Kind::Rpc),
        "llm": state.procs.status(Kind::Llm),
    })))
}

async fn list_models(State(state): State<Shared>) -> Result<Json<Vec<Model>>, ApiError> {
    let cfg = read_config(&state)?;
    let found = tokio::task::spawn_blocking(move || models::scan(&cfg))
        .await
        .map_err(|_| internal("falló la búsqueda de modelos"))?;
    Ok(Json(found))
}

async fn reprobe(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    let report = tokio::task::spawn_blocking(uac_probe::probe)
        .await
        .map_err(|_| internal("falló el sondeo"))?;
    *state
        .report
        .write()
        .map_err(|_| internal("informe bloqueado"))? = Some(report);
    Ok(Json(json!({ "ok": true })))
}

async fn get_config(State(state): State<Shared>) -> Result<Json<Config>, ApiError> {
    Ok(Json(read_config(&state)?))
}

async fn put_config(
    State(state): State<Shared>,
    Json(new_cfg): Json<Config>,
) -> Result<Json<Value>, ApiError> {
    let old_port = read_config(&state)?.panel_port;
    config::save(&state.config_path, &new_cfg)?;
    *state
        .config
        .write()
        .map_err(|_| internal("configuración bloqueada"))? = new_cfg.clone();
    let restart_needed = new_cfg.panel_port != old_port;
    Ok(Json(
        json!({ "ok": true, "restart_needed": restart_needed }),
    ))
}

fn llama_exe(cfg: &Config, name: &str) -> Result<PathBuf, ApiError> {
    let dir = config::resolve_llama_dir(cfg).ok_or_else(|| {
        ApiError(
            StatusCode::BAD_REQUEST,
            "no se encontró llama.cpp; indica su carpeta en la configuración".to_string(),
        )
    })?;
    Ok(dir.join(config::exe_name(name)))
}

#[derive(Deserialize)]
struct RpcStart {
    /// Confirmación explícita para escuchar en una IP de la red local.
    #[serde(default)]
    allow_lan: bool,
}

pub fn rpc_args(cfg: &Config) -> Vec<String> {
    let mut args = vec![
        "-H".to_string(),
        cfg.rpc.bind.clone(),
        "-p".to_string(),
        cfg.rpc.port.to_string(),
    ];
    if !cfg.rpc.device.is_empty() {
        args.push("-d".to_string());
        args.push(cfg.rpc.device.clone());
    }
    if cfg.rpc.cache {
        args.push("-c".to_string());
    }
    args
}

async fn rpc_start(
    State(state): State<Shared>,
    Json(body): Json<RpcStart>,
) -> Result<Json<Value>, ApiError> {
    let cfg = read_config(&state)?;
    cfg.validate()?;
    if !cfg.rpc_is_loopback() && !body.allow_lan {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "el servidor RPC no tiene autenticación: confirma que quieres exponerlo en la red local"
                .to_string(),
        ));
    }
    let exe = llama_exe(&cfg, "ggml-rpc-server")?;
    state.procs.start(Kind::Rpc, &exe, &rpc_args(&cfg))?;
    Ok(Json(json!({ "ok": true })))
}

async fn rpc_stop(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    state.procs.stop(Kind::Rpc)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct LlmStart {
    model_path: PathBuf,
}

pub fn llm_args(cfg: &Config, model: &std::path::Path) -> Vec<String> {
    let mut args = vec![
        "-m".to_string(),
        model.display().to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--port".to_string(),
        cfg.llm.port.to_string(),
        "-ngl".to_string(),
        cfg.llm.gpu_layers.to_string(),
        "-c".to_string(),
        cfg.llm.ctx_size.to_string(),
    ];
    if !cfg.llm.rpc_peers.is_empty() {
        args.push("--rpc".to_string());
        args.push(cfg.llm.rpc_peers.join(","));
    }
    args
}

async fn llm_start(
    State(state): State<Shared>,
    Json(body): Json<LlmStart>,
) -> Result<Json<Value>, ApiError> {
    let cfg = read_config(&state)?;
    cfg.validate()?;
    // Solo se cargan modelos que el propio escaneo encontró, nunca rutas arbitrarias.
    let scan_cfg = cfg.clone();
    let known = tokio::task::spawn_blocking(move || models::scan(&scan_cfg))
        .await
        .map_err(|_| internal("falló la búsqueda de modelos"))?;
    if !known.iter().any(|m| m.path == body.model_path) {
        return Err(ApiError(
            StatusCode::BAD_REQUEST,
            "ese modelo no está en la lista de modelos encontrados".to_string(),
        ));
    }
    let exe = llama_exe(&cfg, "llama-server")?;
    state
        .procs
        .start(Kind::Llm, &exe, &llm_args(&cfg, &body.model_path))?;
    Ok(Json(json!({
        "ok": true,
        "chat_url": format!("http://127.0.0.1:{}/", cfg.llm.port),
    })))
}

async fn llm_stop(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    state.procs.stop(Kind::Llm)?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_hosts() {
        assert!(is_local_host("127.0.0.1:47800"));
        assert!(is_local_host("localhost:47800"));
        assert!(is_local_host("[::1]:47800"));
        assert!(!is_local_host("evil.example:47800"));
        assert!(!is_local_host("127.0.0.1.evil.example"));
    }

    #[test]
    fn rpc_command_line() {
        let mut cfg = Config::default();
        cfg.rpc.device = "Vulkan0".to_string();
        assert_eq!(
            rpc_args(&cfg),
            ["-H", "127.0.0.1", "-p", "50052", "-d", "Vulkan0", "-c"]
        );
    }

    #[test]
    fn llm_command_line_with_peers() {
        let mut cfg = Config::default();
        cfg.llm.rpc_peers = vec!["192.168.1.20:50052".to_string()];
        let args = llm_args(&cfg, std::path::Path::new("m.gguf"));
        assert_eq!(args[..2], ["-m", "m.gguf"]);
        assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(
            args.windows(2)
                .any(|w| w == ["--rpc", "192.168.1.20:50052"])
        );
    }
}
