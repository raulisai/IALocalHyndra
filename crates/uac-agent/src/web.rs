//! Panel web local. Escucha solo en 127.0.0.1.
//!
//! Defensas para una API local que puede arrancar procesos:
//! - la cabecera `Host` debe ser 127.0.0.1 o localhost (evita DNS rebinding);
//! - toda petición que modifica algo exige la cabecera `X-UAC-Panel`. Un sitio
//!   web ajeno no puede enviarla sin un preflight CORS que este servidor no
//!   acepta, así que no puede disparar acciones desde el navegador.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Instant;

use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::middleware::{self, Next};
use axum::response::{Html, IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uac_planner::MemoryPolicy;
use uac_protocol::{BackendStatus, GpuInfo, MemoryKind, NodeReport};

use crate::config::{self, Config};
use crate::discovery::{self, Advert, Context, Discovery, Snapshot};
use crate::models::{self, Model};
use crate::procs::{Kind, Procs};

const INDEX_HTML: &str = include_str!("../static/index.html");
pub const ACTION_HEADER: &str = "x-uac-panel";
const MIB: u64 = 1 << 20;

pub struct AppState {
    pub config: RwLock<Config>,
    pub config_path: PathBuf,
    pub config_warning: Option<String>,
    pub report: RwLock<Option<NodeReport>>,
    pub procs: Procs,
    pub discovery: Discovery,
    /// Nombre del modelo que sirve `llama-server`, si está en marcha.
    pub llm_model: RwLock<Option<String>>,
    /// El modelo en marcha está repartido con otros equipos.
    pub llm_split: AtomicBool,
    /// Dirección en la que el chat está abierto a la red, si lo está.
    pub llm_share: RwLock<Option<SocketAddr>>,
    pub node_id: String,
    pub hostname: String,
    pub started: Instant,
    pub panel_url: String,
}

pub type Shared = Arc<AppState>;

/// GPU principal: la de más memoria que no sea integrada.
pub fn main_gpu(report: &NodeReport) -> Option<&GpuInfo> {
    report
        .gpus
        .iter()
        .filter(|g| g.memory_kind != MemoryKind::Shared)
        .max_by_key(|g| g.vram_bytes.unwrap_or(0))
}

fn memory_label(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Unified => "unified",
        _ => "dedicated",
    }
}

/// Memoria planificable de una GPU descrita en un anuncio.
pub fn usable_for(vram_bytes: u64, memory: &str) -> u64 {
    let kind = if memory == "unified" {
        MemoryKind::Unified
    } else {
        MemoryKind::Dedicated
    };
    uac_planner::usable_bytes(vram_bytes, kind, &MemoryPolicy::default())
}

impl AppState {
    fn cfg(&self) -> Config {
        self.config.read().map(|c| c.clone()).unwrap_or_default()
    }

    /// Lo que este nodo anuncia a la red local.
    pub fn advert(&self) -> Advert {
        let cfg = self.cfg();
        let report = self.report.read().ok().and_then(|r| r.clone());
        let gpu = report.as_ref().and_then(main_gpu);
        // Solo se anuncia un RPC que otros equipos puedan alcanzar. La variable
        // de entorno existe para probar dos instancias en un mismo equipo.
        let reachable =
            !cfg.rpc_is_loopback() || std::env::var_os("UAC_ADVERTISE_LOOPBACK").is_some();
        let rpc = (self.procs.is_running(Kind::Rpc) && reachable)
            .then(|| cfg.rpc.bind.parse::<IpAddr>().ok())
            .flatten()
            .map(|ip| SocketAddr::new(ip, cfg.rpc.port));
        let model = if self.procs.is_running(Kind::Llm) {
            self.llm_model
                .read()
                .ok()
                .and_then(|m| m.clone())
                .unwrap_or_default()
        } else {
            String::new()
        };
        let chat = if self.procs.is_running(Kind::Llm) {
            self.llm_share.read().ok().and_then(|s| *s)
        } else {
            None
        };
        Advert {
            id: self.node_id.clone(),
            name: if cfg.node_name.is_empty() {
                self.hostname.clone()
            } else {
                cfg.node_name.clone()
            },
            version: env!("CARGO_PKG_VERSION").to_string(),
            os: std::env::consts::OS.to_string(),
            gpu: gpu.map(|g| g.name.clone()).unwrap_or_default(),
            vram_bytes: gpu.and_then(|g| g.vram_bytes).unwrap_or(0),
            memory: gpu
                .map(|g| memory_label(g.memory_kind))
                .unwrap_or("dedicated")
                .to_string(),
            backend: gpu
                .and_then(|g| {
                    g.backends.iter().find(|b| {
                        matches!(
                            b.status,
                            BackendStatus::Verified
                                | BackendStatus::Detected
                                | BackendStatus::Expected
                        )
                    })
                })
                .map(|b| format!("{:?}", b.backend).to_lowercase())
                .unwrap_or_default(),
            rpc,
            chat,
            model,
        }
    }

    pub fn discovery_context(&self) -> Context {
        let cfg = self.cfg();
        let mut targets: Vec<SocketAddr> = cfg
            .llm
            .rpc_peers
            .iter()
            .filter_map(|p| p.parse().ok())
            .collect();
        for peer in self.discovery.snapshot().peers {
            if let (true, Some(rpc)) = (peer.online, peer.advert.rpc) {
                if !targets.contains(&rpc) {
                    targets.push(rpc);
                }
            }
        }
        Context {
            advert: self.advert(),
            port: cfg.panel_port,
            enabled: cfg.discovery,
            notify: cfg.notifications,
            targets,
            probe: !self.procs.is_running(Kind::Llm),
        }
    }
}

pub fn router(state: Shared) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/api/status", get(status))
        .route("/api/models", get(list_models))
        .route("/api/probe", post(reprobe))
        .route("/api/scan", post(scan))
        .route("/api/config", get(get_config).put(put_config))
        .route("/api/peers/use", post(peer_use))
        .route("/api/peers/unuse", post(peer_unuse))
        .route("/api/peers/add", post(peer_add))
        .route("/api/rpc/start", post(rpc_start))
        .route("/api/rpc/stop", post(rpc_stop))
        .route("/api/llm/start", post(llm_start))
        .route("/api/llm/stop", post(llm_stop))
        .route("/api/llm/rotate-key", post(rotate_key))
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

fn bad(msg: impl Into<String>) -> ApiError {
    ApiError(StatusCode::BAD_REQUEST, msg.into())
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

/// Aplica un cambio a la configuración, la valida, la guarda y la activa.
fn update_config(state: &AppState, change: impl FnOnce(&mut Config)) -> Result<Config, ApiError> {
    let mut cfg = read_config(state)?;
    change(&mut cfg);
    config::save(&state.config_path, &cfg)?;
    *state
        .config
        .write()
        .map_err(|_| internal("configuración bloqueada"))? = cfg.clone();
    Ok(cfg)
}

async fn index() -> impl IntoResponse {
    ([(header::CACHE_CONTROL, "no-store")], Html(INDEX_HTML))
}

/// Un equipo tal como lo muestra el panel.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PeerView {
    pub id: String,
    pub name: String,
    pub version: String,
    pub os: String,
    pub gpu: String,
    pub vram_bytes: u64,
    pub usable_bytes: u64,
    pub backend: String,
    pub rpc: Option<String>,
    /// Chat de ese equipo abierto a la red (necesita su clave).
    pub chat: Option<String>,
    pub model: String,
    pub addr: Option<String>,
    pub online: bool,
    pub seen_secs_ago: Option<u64>,
    /// `auto` (descubierto en la red) o `manual` (añadido por IP).
    pub source: &'static str,
    /// Su GPU se usará al cargar un modelo en este nodo.
    pub in_use: bool,
    pub rpc_reachable: Option<bool>,
    pub rpc_latency_ms: Option<f64>,
}

/// Une los equipos descubiertos con los añadidos a mano en la configuración.
pub fn peer_views(snap: &Snapshot, cfg: &Config) -> Vec<PeerView> {
    let in_use: Vec<SocketAddr> = cfg
        .llm
        .rpc_peers
        .iter()
        .filter_map(|p| p.parse().ok())
        .collect();
    let mut views: Vec<PeerView> = snap
        .peers
        .iter()
        .map(|p| {
            let probe = p.advert.rpc.and_then(|rpc| snap.probes.get(&rpc));
            PeerView {
                id: p.advert.id.clone(),
                name: p.advert.label(),
                version: p.advert.version.clone(),
                os: p.advert.os.clone(),
                gpu: p.advert.gpu.clone(),
                vram_bytes: p.advert.vram_bytes,
                usable_bytes: usable_for(p.advert.vram_bytes, &p.advert.memory),
                backend: p.advert.backend.clone(),
                rpc: p.advert.rpc.map(|a| a.to_string()),
                chat: p.advert.chat.map(|a| a.to_string()),
                model: p.advert.model.clone(),
                addr: p.addr.map(|a| a.to_string()),
                online: p.online,
                seen_secs_ago: Some(p.seen_secs_ago),
                source: "auto",
                in_use: p.advert.rpc.is_some_and(|rpc| in_use.contains(&rpc)),
                rpc_reachable: probe.map(|pr| pr.reachable),
                rpc_latency_ms: probe.and_then(|pr| pr.latency_ms),
            }
        })
        .collect();

    let advertised: Vec<SocketAddr> = snap.peers.iter().filter_map(|p| p.advert.rpc).collect();
    for addr in in_use.iter().filter(|a| !advertised.contains(a)) {
        let probe = snap.probes.get(addr);
        views.push(PeerView {
            id: format!("manual-{addr}"),
            name: addr.ip().to_string(),
            version: String::new(),
            os: String::new(),
            gpu: String::new(),
            vram_bytes: 0,
            usable_bytes: 0,
            backend: String::new(),
            rpc: Some(addr.to_string()),
            chat: None,
            model: String::new(),
            addr: Some(addr.ip().to_string()),
            online: probe.is_some_and(|pr| pr.reachable),
            seen_secs_ago: None,
            source: "manual",
            in_use: true,
            rpc_reachable: probe.map(|pr| pr.reachable),
            rpc_latency_ms: probe.and_then(|pr| pr.latency_ms),
        });
    }
    views
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
    let advert = state.advert();
    let snap = state.discovery.snapshot();
    let peers = peer_views(&snap, &cfg);
    Ok(Json(json!({
        "version": env!("CARGO_PKG_VERSION"),
        "node_name": advert.name,
        "uptime_secs": state.started.elapsed().as_secs(),
        "panel_url": state.panel_url,
        "config_path": state.config_path,
        "config_warning": state.config_warning,
        "config": cfg,
        "llama": { "dir": llama_dir, "tools": tools },
        "report": report,
        "self": {
            "id": advert.id,
            "gpu": advert.gpu,
            "vram_bytes": advert.vram_bytes,
            "usable_bytes": usable_for(advert.vram_bytes, &advert.memory),
            "backend": advert.backend,
            "sharing": advert.rpc.map(|a| a.to_string()),
            "chat": advert.chat.map(|a| a.to_string()),
            "model": advert.model,
        },
        "addresses": discovery::local_ipv4(),
        "discovery": {
            "enabled": snap.enabled,
            "running": snap.running,
            "scanning": snap.scanning,
            "error": snap.error,
        },
        "peers": peers,
        "events": snap.events,
        "rpc": state.procs.status(Kind::Rpc),
        "llm": state.procs.status(Kind::Llm),
        "llm_split": state.llm_split.load(Ordering::Relaxed),
    })))
}

/// Memoria que necesita un modelo: sus pesos más un margen para el contexto
/// y los búferes de cómputo. Es una estimación, no una medición.
pub fn required_bytes(model_size: u64) -> u64 {
    model_size + model_size / 7 + 512 * MIB
}

/// Dónde cabría un modelo: `local`, `split` (repartido con las GPU en uso),
/// `too_big` o `unknown` si aún no se conoce la GPU de este equipo.
pub fn fit_kind(required: u64, local_usable: Option<u64>, remote_usable: u64) -> &'static str {
    match local_usable {
        None => "unknown",
        Some(local) if required <= local => "local",
        Some(local) if remote_usable > 0 && required <= local + remote_usable => "split",
        Some(_) => "too_big",
    }
}

#[derive(Serialize)]
struct ModelView {
    #[serde(flatten)]
    model: Model,
    required_bytes: u64,
    fit: &'static str,
}

async fn list_models(State(state): State<Shared>) -> Result<Json<Vec<ModelView>>, ApiError> {
    let cfg = read_config(&state)?;
    let scan_cfg = cfg.clone();
    let found = tokio::task::spawn_blocking(move || models::scan(&scan_cfg))
        .await
        .map_err(|_| internal("falló la búsqueda de modelos"))?;
    let advert = state.advert();
    let local = (advert.vram_bytes > 0).then(|| usable_for(advert.vram_bytes, &advert.memory));
    let remote: u64 = peer_views(&state.discovery.snapshot(), &cfg)
        .iter()
        .filter(|p| p.in_use && p.online)
        .map(|p| p.usable_bytes)
        .sum();
    Ok(Json(
        found
            .into_iter()
            .map(|model| {
                let required = required_bytes(model.size_bytes);
                ModelView {
                    fit: fit_kind(required, local, remote),
                    required_bytes: required,
                    model,
                }
            })
            .collect(),
    ))
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

async fn scan(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    let snap = state.discovery.snapshot();
    if !snap.enabled {
        return Err(bad(
            "la búsqueda automática está desactivada en la configuración",
        ));
    }
    if !snap.running {
        return Err(bad(snap.error.unwrap_or_else(|| {
            "la búsqueda en la red no está en marcha".to_string()
        })));
    }
    state.discovery.request_scan();
    Ok(Json(json!({ "ok": true })))
}

async fn get_config(State(state): State<Shared>) -> Result<Json<Config>, ApiError> {
    Ok(Json(read_config(&state)?))
}

async fn put_config(
    State(state): State<Shared>,
    Json(new_cfg): Json<Config>,
) -> Result<Json<Value>, ApiError> {
    let old = read_config(&state)?;
    let restart_needed = new_cfg.panel_port != old.panel_port || new_cfg.discovery != old.discovery;
    update_config(&state, |cfg| *cfg = new_cfg)?;
    Ok(Json(
        json!({ "ok": true, "restart_needed": restart_needed }),
    ))
}

#[derive(Deserialize)]
struct PeerAddr {
    addr: String,
}

fn set_peer_use(state: &AppState, addr: SocketAddr, used: bool) -> Result<(), ApiError> {
    update_config(state, |cfg| {
        cfg.llm
            .rpc_peers
            .retain(|p| p.parse::<SocketAddr>().ok() != Some(addr));
        if used {
            cfg.llm.rpc_peers.push(addr.to_string());
        }
    })?;
    Ok(())
}

async fn peer_use(
    State(state): State<Shared>,
    Json(body): Json<PeerAddr>,
) -> Result<Json<Value>, ApiError> {
    let addr = config::parse_lan_socket(&body.addr)?;
    set_peer_use(&state, addr, true)?;
    Ok(Json(json!({ "ok": true })))
}

async fn peer_unuse(
    State(state): State<Shared>,
    Json(body): Json<PeerAddr>,
) -> Result<Json<Value>, ApiError> {
    let addr = config::parse_lan_socket(&body.addr)?;
    set_peer_use(&state, addr, false)?;
    Ok(Json(json!({ "ok": true })))
}

/// Añade a mano un servidor RPC (p. ej. un equipo que solo tiene llama.cpp).
/// Solo se acepta si responde ahora mismo.
async fn peer_add(
    State(state): State<Shared>,
    Json(body): Json<PeerAddr>,
) -> Result<Json<Value>, ApiError> {
    let addr = config::parse_lan_socket(&body.addr)?;
    let result = tokio::task::spawn_blocking(move || discovery::probe(addr))
        .await
        .map_err(|_| internal("falló la comprobación"))?;
    if !result.reachable {
        return Err(bad(format!(
            "{addr} no responde. Comprueba la IP, que el servidor RPC esté iniciado en ese equipo y que su firewall permita el puerto."
        )));
    }
    state.discovery.set_probe(addr, result);
    set_peer_use(&state, addr, true)?;
    state
        .discovery
        .push_event("info", format!("Se añadió {addr} a mano"));
    Ok(Json(json!({ "ok": true, "latency_ms": result.latency_ms })))
}

fn llama_exe(cfg: &Config, name: &str) -> Result<PathBuf, ApiError> {
    let dir = config::resolve_llama_dir(cfg)
        .ok_or_else(|| bad("no se encontró llama.cpp; indica su carpeta en la configuración"))?;
    Ok(dir.join(config::exe_name(name)))
}

#[derive(Deserialize)]
struct RpcStart {
    /// IP en la que escuchar; si falta, la de la configuración.
    bind: Option<String>,
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
    let cfg = match body.bind {
        Some(bind) => update_config(&state, |cfg| cfg.rpc.bind = bind.trim().to_string())?,
        None => read_config(&state)?,
    };
    cfg.validate()?;
    if !cfg.rpc_is_loopback() && !body.allow_lan {
        return Err(bad(
            "el servidor RPC no tiene autenticación: confirma que quieres exponerlo en la red local",
        ));
    }
    let exe = llama_exe(&cfg, "ggml-rpc-server")?;
    state.procs.start(Kind::Rpc, &exe, &rpc_args(&cfg), &[])?;
    Ok(Json(json!({ "ok": true })))
}

async fn rpc_stop(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    state.procs.stop(Kind::Rpc)?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct LlmStart {
    model_path: PathBuf,
    /// IP de este equipo por la que abrir el chat a la red. Sin ella, solo local.
    #[serde(default)]
    share_bind: Option<String>,
    /// Confirmación explícita para abrir el chat a la red local.
    #[serde(default)]
    allow_lan: bool,
}

/// `peers`: servidores RPC entre los que repartir el modelo (vacío = solo aquí).
/// `host`: IP en la que escuchar. La clave de acceso no va aquí: se pasa por
/// la variable `LLAMA_API_KEY` para que no se vea en la lista de procesos.
pub fn llm_args(
    cfg: &Config,
    model: &std::path::Path,
    peers: &[String],
    host: IpAddr,
) -> Vec<String> {
    let mut args = vec![
        "-m".to_string(),
        model.display().to_string(),
        "--host".to_string(),
        host.to_string(),
        "--port".to_string(),
        cfg.llm.port.to_string(),
        "-ngl".to_string(),
        cfg.llm.gpu_layers.to_string(),
        "-c".to_string(),
        cfg.llm.ctx_size.to_string(),
    ];
    if !peers.is_empty() {
        args.push("--rpc".to_string());
        args.push(peers.join(","));
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
    let Some(model) = known.iter().find(|m| m.path == body.model_path) else {
        return Err(bad("ese modelo no está en la lista de modelos encontrados"));
    };
    // Regla del planner: si el modelo cabe en esta GPU no se reparte, porque
    // cada salto de red cuesta velocidad.
    let advert = state.advert();
    let local = (advert.vram_bytes > 0).then(|| usable_for(advert.vram_bytes, &advert.memory));
    let fits_here = fit_kind(required_bytes(model.size_bytes), local, 0) == "local";
    // Solo se reparte con los equipos que responden ahora: uno apagado haría
    // fallar la carga entera.
    let peers = if fits_here {
        Vec::new()
    } else {
        let candidates = cfg.llm.rpc_peers.clone();
        tokio::task::spawn_blocking(move || {
            candidates
                .into_iter()
                .filter(|p| {
                    p.parse::<SocketAddr>()
                        .is_ok_and(|addr| discovery::probe(addr).reachable)
                })
                .collect::<Vec<String>>()
        })
        .await
        .map_err(|_| internal("falló la comprobación de equipos"))?
    };
    let split = !peers.is_empty();
    let skipped = if fits_here {
        0
    } else {
        cfg.llm.rpc_peers.len() - peers.len()
    };
    // Abrir el chat a la red: solo por una IP de este equipo en red local, con
    // confirmación explícita y siempre con clave.
    let share = match &body.share_bind {
        None => None,
        Some(text) => {
            let ip: IpAddr = text
                .trim()
                .parse()
                .map_err(|_| bad(format!("«{text}» no es una IP válida")))?;
            let mine = discovery::local_ipv4();
            if ip.is_loopback()
                || !config::is_lan_ip(ip)
                || !mine.iter().any(|a| IpAddr::V4(a.ip) == ip)
            {
                return Err(bad(
                    "solo se puede compartir el chat por una conexión de red local de este equipo",
                ));
            }
            if !body.allow_lan {
                return Err(bad(
                    "confirma que quieres abrir el chat a otros equipos de tu red local",
                ));
            }
            Some(ip)
        }
    };
    let mut cfg = cfg;
    if share.is_some() && cfg.llm.api_key.is_empty() {
        let key = config::new_api_key()?;
        cfg = update_config(&state, |c| c.llm.api_key = key)?;
    }
    let host = share.unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let key = cfg.llm.api_key.clone();
    let envs: Vec<(&str, &str)> = if share.is_some() {
        vec![("LLAMA_API_KEY", key.as_str())]
    } else {
        Vec::new()
    };
    let exe = llama_exe(&cfg, "llama-server")?;
    state.procs.start(
        Kind::Llm,
        &exe,
        &llm_args(&cfg, &body.model_path, &peers, host),
        &envs,
    )?;
    if let Ok(mut slot) = state.llm_share.write() {
        *slot = share.map(|ip| SocketAddr::new(ip, cfg.llm.port));
    }
    state.llm_split.store(split, Ordering::Relaxed);
    if let Ok(mut slot) = state.llm_model.write() {
        *slot = Some(model.name.clone());
    }
    Ok(Json(json!({
        "ok": true,
        "split": split,
        "peers_skipped": skipped,
        "chat_url": format!("http://127.0.0.1:{}/", cfg.llm.port),
    })))
}

/// Genera una clave nueva. Quien tuviera la anterior pierde el acceso en cuanto
/// se reinicie el chat.
async fn rotate_key(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    let key = config::new_api_key()?;
    update_config(&state, |c| c.llm.api_key = key)?;
    Ok(Json(json!({
        "ok": true,
        "restart_needed": state.procs.is_running(Kind::Llm),
    })))
}

async fn llm_stop(State(state): State<Shared>) -> Result<Json<Value>, ApiError> {
    state.procs.stop(Kind::Llm)?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::discovery::{PeerSnapshot, Probe};

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
        let loopback = IpAddr::from([127, 0, 0, 1]);
        let local_only = llm_args(&cfg, std::path::Path::new("m.gguf"), &[], loopback);
        assert!(!local_only.iter().any(|a| a == "--rpc"));
        let args = llm_args(
            &cfg,
            std::path::Path::new("m.gguf"),
            &cfg.llm.rpc_peers,
            loopback,
        );
        // La clave nunca viaja como argumento.
        assert!(!args.iter().any(|a| a.contains("api-key")));
        assert_eq!(args[..2], ["-m", "m.gguf"]);
        assert!(args.windows(2).any(|w| w == ["--host", "127.0.0.1"]));
        assert!(
            args.windows(2)
                .any(|w| w == ["--rpc", "192.168.1.20:50052"])
        );
    }

    #[test]
    fn where_a_model_fits() {
        let gib = 1u64 << 30;
        assert_eq!(fit_kind(5 * gib, Some(7 * gib), 0), "local");
        assert_eq!(fit_kind(12 * gib, Some(7 * gib), 7 * gib), "split");
        assert_eq!(fit_kind(12 * gib, Some(7 * gib), 0), "too_big");
        assert_eq!(fit_kind(20 * gib, Some(7 * gib), 7 * gib), "too_big");
        assert_eq!(fit_kind(gib, None, 0), "unknown");
        // 4,36 GiB de pesos necesitan algo más de 5 GiB.
        let need = required_bytes(4_680_000_000);
        assert!(need > 5 * gib && need < 6 * gib, "{need}");
    }

    #[test]
    fn merges_discovered_and_manual_peers() {
        let rpc: SocketAddr = "192.168.1.20:50052".parse().unwrap();
        let manual: SocketAddr = "192.168.1.30:50052".parse().unwrap();
        let mut snap = Snapshot::default();
        snap.peers.push(PeerSnapshot {
            advert: Advert {
                id: "mac-47800".to_string(),
                name: "Mac".to_string(),
                vram_bytes: 24 << 30,
                memory: "unified".to_string(),
                rpc: Some(rpc),
                ..Advert::default()
            },
            addr: Some(rpc.ip()),
            online: true,
            seen_secs_ago: 1,
        });
        snap.probes.insert(
            rpc,
            Probe {
                reachable: true,
                latency_ms: Some(0.4),
            },
        );
        let mut cfg = Config::default();
        cfg.llm.rpc_peers = vec![rpc.to_string(), manual.to_string()];

        let views = peer_views(&snap, &cfg);
        assert_eq!(views.len(), 2);
        assert_eq!(views[0].source, "auto");
        assert!(views[0].in_use);
        assert_eq!(views[0].rpc_latency_ms, Some(0.4));
        // Memoria unificada: se reserva más que en una GPU dedicada.
        assert!(views[0].usable_bytes < 24 << 30 && views[0].usable_bytes > 12 << 30);
        assert_eq!(views[1].source, "manual");
        assert!(
            !views[1].online,
            "sin sondeo todavía no se da por conectado"
        );

        cfg.llm.rpc_peers.clear();
        let views = peer_views(&snap, &cfg);
        assert_eq!(views.len(), 1);
        assert!(!views[0].in_use);
    }
}
