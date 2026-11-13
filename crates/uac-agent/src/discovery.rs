//! Descubrimiento de otros nodos en la red local con mDNS/DNS-SD.
//!
//! Cada nodo anuncia el servicio `_uac._tcp.local.` con un registro TXT que
//! resume qué es y qué ofrece. No se abre ningún puerto TCP nuevo: todo lo que
//! un nodo sabe de otro viaja en ese anuncio. Lo anunciado es visible para
//! cualquiera en la red local, así que solo incluye datos no sensibles.

use std::collections::{BTreeMap, VecDeque};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use mdns_sd::{IfKind, IfPredicate, ServiceDaemon, ServiceEvent, ServiceInfo};
use serde::Serialize;

pub const SERVICE_TYPE: &str = "_uac._tcp.local.";
const PROTO: &str = "1";
const MAX_EVENTS: usize = 50;
const MAX_FIELD_CHARS: usize = 120;
const PROBE_EVERY: Duration = Duration::from_secs(10);
const VERIFY_EVERY: Duration = Duration::from_secs(15);
const PROBE_TIMEOUT: Duration = Duration::from_millis(400);
const SCAN_SHOWN_FOR: Duration = Duration::from_secs(4);

/// Lo que un nodo anuncia de sí mismo.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Advert {
    pub id: String,
    pub name: String,
    pub version: String,
    pub os: String,
    pub gpu: String,
    pub vram_bytes: u64,
    /// `dedicated` o `unified`.
    pub memory: String,
    pub backend: String,
    /// Servidor RPC de llama.cpp alcanzable desde la red, si está en marcha.
    pub rpc: Option<SocketAddr>,
    /// Chat (llama-server) abierto a la red con clave, si lo hay. La clave
    /// nunca se anuncia: se la da el usuario a quien quiera usarlo.
    pub chat: Option<SocketAddr>,
    /// Modelo cargado ahora mismo, o vacío.
    pub model: String,
}

/// Dirección anunciada por otro equipo. Es un dato no confiable: solo se
/// acepta si es de red local, para que un anuncio falso no nos haga conectar
/// o abrir una dirección de Internet.
fn lan_addr(value: Option<String>) -> Option<SocketAddr> {
    value
        .and_then(|v| v.parse::<SocketAddr>().ok())
        .filter(|a| crate::config::is_lan_ip(a.ip()))
}

fn clip(text: &str) -> String {
    text.chars().take(MAX_FIELD_CHARS).collect()
}

impl Advert {
    pub fn to_txt(&self) -> Vec<(&'static str, String)> {
        vec![
            ("proto", PROTO.to_string()),
            ("id", clip(&self.id)),
            ("name", clip(&self.name)),
            ("ver", clip(&self.version)),
            ("os", clip(&self.os)),
            ("gpu", clip(&self.gpu)),
            ("vram", self.vram_bytes.to_string()),
            ("mem", clip(&self.memory)),
            ("backend", clip(&self.backend)),
            ("rpc", self.rpc.map(|a| a.to_string()).unwrap_or_default()),
            ("chat", self.chat.map(|a| a.to_string()).unwrap_or_default()),
            ("model", clip(&self.model)),
        ]
    }

    /// Reconstruye un anuncio. Devuelve `None` si es de otro protocolo o no
    /// trae identificador.
    pub fn from_txt(get: impl Fn(&str) -> Option<String>) -> Option<Self> {
        if get("proto").as_deref() != Some(PROTO) {
            return None;
        }
        let id = get("id").filter(|id| !id.is_empty())?;
        let text = |key: &str| get(key).map(|v| clip(&v)).unwrap_or_default();
        Some(Self {
            id: clip(&id),
            name: text("name"),
            version: text("ver"),
            os: text("os"),
            gpu: text("gpu"),
            vram_bytes: get("vram").and_then(|v| v.parse().ok()).unwrap_or(0),
            memory: text("mem"),
            backend: text("backend"),
            rpc: lan_addr(get("rpc")),
            chat: lan_addr(get("chat")),
            model: text("model"),
        })
    }

    pub fn label(&self) -> String {
        if self.name.is_empty() {
            self.id.clone()
        } else {
            self.name.clone()
        }
    }
}

/// Identificador estable de una instancia: nombre del equipo + puerto del panel.
pub fn node_id(hostname: &str, panel_port: u16) -> String {
    let mut clean: String = hostname
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    clean.truncate(40);
    let clean = clean.trim_matches('-');
    let base = if clean.is_empty() { "nodo" } else { clean };
    format!("{base}-{panel_port}")
}

/// Interfaces que no llevan a otros equipos: virtuales, túneles, etc.
pub fn is_virtual_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "vethernet",
        "virtual",
        "vmware",
        "vbox",
        "docker",
        "wsl",
        "loopback",
        "bluetooth",
        "virbr",
        "veth",
        "br-",
        "utun",
        "awdl",
        "llw",
        "tailscale",
        "zerotier",
    ]
    .iter()
    .any(|pat| n.contains(pat))
}

/// Interfaces inalámbricas: sirven, pero el cable es más rápido y estable.
pub fn is_wifi_name(name: &str) -> bool {
    let n = name.to_ascii_lowercase();
    [
        "wi-fi",
        "wifi",
        "wireless",
        "wlan",
        "inalámbrica",
        "airport",
    ]
    .iter()
    .any(|pat| n.contains(pat))
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LocalAddr {
    pub name: String,
    pub ip: Ipv4Addr,
    #[serde(skip)]
    pub netmask: Ipv4Addr,
}

/// Direcciones IPv4 de las interfaces físicas activas.
pub fn local_ipv4() -> Vec<LocalAddr> {
    let Ok(interfaces) = if_addrs::get_if_addrs() else {
        return Vec::new();
    };
    let mut found: Vec<LocalAddr> = interfaces
        .into_iter()
        .filter(|i| i.is_oper_up() && !i.is_loopback() && !is_virtual_name(&i.name))
        .filter_map(|i| match i.addr {
            if_addrs::IfAddr::V4(v4) => Some(LocalAddr {
                name: i.name,
                ip: v4.ip,
                netmask: v4.netmask,
            }),
            _ => None,
        })
        .collect();
    found.sort_by(|a, b| a.name.cmp(&b.name).then(a.ip.cmp(&b.ip)));
    found
}

/// IP de este equipo por la que compartir la GPU sin preguntar: la de una
/// conexión con cable si la hay, y si no la de Wi-Fi.
pub fn preferred_lan_ip(local: &[LocalAddr]) -> Option<Ipv4Addr> {
    local
        .iter()
        .find(|l| !is_wifi_name(&l.name) && !l.ip.is_link_local())
        .or_else(|| local.iter().find(|l| !is_wifi_name(&l.name)))
        .or_else(|| local.first())
        .map(|l| l.ip)
}

fn same_subnet(a: Ipv4Addr, b: Ipv4Addr, mask: Ipv4Addr) -> bool {
    u32::from(a) & u32::from(mask) == u32::from(b) & u32::from(mask)
}

/// Elige la dirección de un vecino por la que es más probable alcanzarlo:
/// primero una IPv4 en la misma subred que alguna interfaz propia, y entre
/// ellas la de una interfaz con cable antes que la de Wi-Fi.
pub fn pick_addr(candidates: &[IpAddr], local: &[LocalAddr]) -> Option<IpAddr> {
    let mut v4: Vec<Ipv4Addr> = candidates
        .iter()
        .filter_map(|ip| match ip {
            IpAddr::V4(v4) => Some(*v4),
            IpAddr::V6(_) => None,
        })
        .collect();
    v4.sort();
    let routable = |ip: &&Ipv4Addr| !ip.is_loopback();
    let on_link = |wifi: bool| {
        v4.iter().filter(routable).find(|ip| {
            local
                .iter()
                .filter(|l| is_wifi_name(&l.name) == wifi)
                .any(|l| same_subnet(**ip, l.ip, l.netmask))
        })
    };
    on_link(false)
        .or_else(|| on_link(true))
        .or_else(|| v4.iter().find(routable))
        .or_else(|| v4.first())
        .map(|ip| IpAddr::V4(*ip))
        .or_else(|| candidates.first().copied())
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize)]
pub struct Probe {
    pub reachable: bool,
    pub latency_ms: Option<f64>,
}

/// Mide cuánto tarda en abrirse una conexión TCP. No habla el protocolo RPC.
pub fn probe(addr: SocketAddr) -> Probe {
    let start = Instant::now();
    match TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) {
        Ok(_) => Probe {
            reachable: true,
            latency_ms: Some((start.elapsed().as_secs_f64() * 10_000.0).round() / 10.0),
        },
        Err(_) => Probe {
            reachable: false,
            latency_ms: None,
        },
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Event {
    pub id: u64,
    pub unix_secs: u64,
    /// `joined`, `left`, `rpc_up`, `rpc_down` o `info`.
    pub kind: &'static str,
    pub text: String,
}

struct PeerState {
    advert: Advert,
    fullname: String,
    addr: Option<IpAddr>,
    online: bool,
    last_seen: Instant,
}

#[derive(Debug, Clone, Serialize)]
pub struct PeerSnapshot {
    pub advert: Advert,
    pub addr: Option<IpAddr>,
    pub online: bool,
    pub seen_secs_ago: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Snapshot {
    pub peers: Vec<PeerSnapshot>,
    pub events: Vec<Event>,
    pub probes: BTreeMap<SocketAddr, Probe>,
    pub enabled: bool,
    pub running: bool,
    pub scanning: bool,
    pub error: Option<String>,
}

#[derive(Default)]
struct Inner {
    peers: BTreeMap<String, PeerState>,
    events: VecDeque<Event>,
    next_event_id: u64,
    probes: BTreeMap<SocketAddr, Probe>,
    enabled: bool,
    running: bool,
    scanning_until: Option<Instant>,
    error: Option<String>,
}

/// Lo que el bucle de descubrimiento necesita saber del resto del programa.
pub struct Context {
    pub advert: Advert,
    pub port: u16,
    pub enabled: bool,
    pub notify: bool,
    /// Servidores RPC cuya disponibilidad hay que comprobar.
    pub targets: Vec<SocketAddr>,
    /// Falso mientras este nodo usa los RPC: no se les molesta con sondeos.
    pub probe: bool,
}

#[derive(Default)]
pub struct Discovery {
    inner: Mutex<Inner>,
    rescan: AtomicBool,
    stop: AtomicBool,
}

impl Discovery {
    fn with<R>(&self, f: impl FnOnce(&mut Inner) -> R) -> Option<R> {
        self.inner.lock().ok().map(|mut inner| f(&mut inner))
    }

    pub fn snapshot(&self) -> Snapshot {
        self.with(|inner| Snapshot {
            peers: inner
                .peers
                .values()
                .map(|p| PeerSnapshot {
                    advert: p.advert.clone(),
                    addr: p.addr,
                    online: p.online,
                    seen_secs_ago: p.last_seen.elapsed().as_secs(),
                })
                .collect(),
            events: inner.events.iter().cloned().collect(),
            probes: inner.probes.clone(),
            enabled: inner.enabled,
            running: inner.running,
            scanning: inner.scanning_until.is_some_and(|t| Instant::now() < t),
            error: inner.error.clone(),
        })
        .unwrap_or_default()
    }

    pub fn online_count(&self) -> usize {
        self.with(|inner| inner.peers.values().filter(|p| p.online).count())
            .unwrap_or(0)
    }

    pub fn request_scan(&self) {
        self.rescan.store(true, Ordering::Relaxed);
        self.with(|inner| inner.scanning_until = Some(Instant::now() + SCAN_SHOWN_FOR));
    }

    pub fn shutdown(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn push_event(&self, kind: &'static str, text: String) {
        self.with(|inner| push_event(inner, kind, text));
    }

    pub fn set_probe(&self, addr: SocketAddr, result: Probe) {
        self.with(|inner| inner.probes.insert(addr, result));
    }
}

fn push_event(inner: &mut Inner, kind: &'static str, text: String) {
    inner.next_event_id += 1;
    if inner.events.len() >= MAX_EVENTS {
        inner.events.pop_front();
    }
    inner.events.push_back(Event {
        id: inner.next_event_id,
        unix_secs: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0),
        kind,
        text,
    });
}

fn fullname(id: &str) -> String {
    format!("{id}.{SERVICE_TYPE}")
}

fn service_info(advert: &Advert, port: u16) -> mdns_sd::Result<ServiceInfo> {
    let txt = advert.to_txt();
    let host = format!("{}.local.", advert.id);
    Ok(ServiceInfo::new(SERVICE_TYPE, &advert.id, &host, "", port, &txt[..])?.enable_addr_auto())
}

fn start_daemon() -> mdns_sd::Result<ServiceDaemon> {
    let daemon = ServiceDaemon::new()?;
    daemon.disable_interface(IfKind::Predicate(IfPredicate::new(|i| {
        is_virtual_name(&i.name)
    })))?;
    Ok(daemon)
}

/// Qué cambió al recibir un anuncio, para avisar al usuario.
#[derive(Debug, PartialEq, Eq)]
enum Change {
    Joined,
    RpcUp,
    RpcDown,
}

fn apply_resolved(
    inner: &mut Inner,
    advert: Advert,
    name: String,
    addr: Option<IpAddr>,
) -> Vec<Change> {
    let mut changes = Vec::new();
    match inner.peers.get_mut(&advert.id) {
        Some(peer) => {
            if !peer.online {
                changes.push(Change::Joined);
            } else if peer.advert.rpc.is_none() && advert.rpc.is_some() {
                changes.push(Change::RpcUp);
            } else if peer.advert.rpc.is_some() && advert.rpc.is_none() {
                changes.push(Change::RpcDown);
            }
            peer.advert = advert;
            peer.fullname = name;
            peer.addr = addr.or(peer.addr);
            peer.online = true;
            peer.last_seen = Instant::now();
        }
        None => {
            changes.push(Change::Joined);
            inner.peers.insert(
                advert.id.clone(),
                PeerState {
                    advert,
                    fullname: name,
                    addr,
                    online: true,
                    last_seen: Instant::now(),
                },
            );
        }
    }
    changes
}

fn handle_event(disc: &Discovery, event: ServiceEvent, own_id: &str, notify: bool) {
    match event {
        ServiceEvent::ServiceResolved(svc) => {
            let Some(advert) =
                Advert::from_txt(|key| svc.get_property_val_str(key).map(str::to_string))
            else {
                return;
            };
            if advert.id == own_id {
                return;
            }
            let candidates: Vec<IpAddr> =
                svc.get_addresses().iter().map(|a| a.to_ip_addr()).collect();
            let addr = pick_addr(&candidates, &local_ipv4());
            let label = advert.label();
            let gpu = advert.gpu.clone();
            let name = svc.get_fullname().to_string();
            let changes = disc
                .with(|inner| {
                    let changes = apply_resolved(inner, advert, name, addr);
                    for change in &changes {
                        let (kind, text) = match change {
                            Change::Joined => ("joined", format!("{label} se conectó")),
                            Change::RpcUp => ("rpc_up", format!("{label} comparte su GPU")),
                            Change::RpcDown => {
                                ("rpc_down", format!("{label} dejó de compartir su GPU"))
                            }
                        };
                        push_event(inner, kind, text);
                    }
                    changes
                })
                .unwrap_or_default();
            if notify && changes.contains(&Change::Joined) {
                let body = if gpu.is_empty() {
                    label
                } else {
                    format!("{label} · {gpu}")
                };
                crate::notify("Equipo conectado al clúster", &body);
            }
        }
        ServiceEvent::ServiceRemoved(_, name) => {
            let left = disc
                .with(|inner| {
                    let peer = inner
                        .peers
                        .values_mut()
                        .find(|p| p.fullname == name && p.online)?;
                    peer.online = false;
                    let label = peer.advert.label();
                    push_event(inner, "left", format!("{label} se desconectó"));
                    Some(label)
                })
                .flatten();
            if let (true, Some(label)) = (notify, left) {
                crate::notify("Equipo desconectado", &label);
            }
        }
        _ => {}
    }
}

/// Bucle de descubrimiento. Bloquea hasta que se llama a [`Discovery::shutdown`].
pub fn run(disc: &Discovery, ctx: impl Fn() -> Context) {
    let first = ctx();
    let daemon = if first.enabled {
        match start_daemon() {
            Ok(daemon) => Some(daemon),
            Err(err) => {
                let msg = format!("no se pudo iniciar el descubrimiento en la red: {err}");
                crate::log_line(&msg);
                disc.with(|inner| inner.error = Some(msg));
                None
            }
        }
    } else {
        None
    };
    disc.with(|inner| {
        inner.enabled = first.enabled;
        inner.running = daemon.is_some();
    });

    let mut receiver = daemon.as_ref().and_then(|d| d.browse(SERVICE_TYPE).ok());
    let mut registered: Option<Advert> = None;
    let mut last_probe: Option<Instant> = None;
    let mut last_verify = Instant::now();

    while !disc.stop.load(Ordering::Relaxed) {
        let c = ctx();

        if let Some(d) = &daemon {
            if registered.as_ref() != Some(&c.advert) {
                match service_info(&c.advert, c.port).and_then(|info| d.register(info)) {
                    Ok(()) => registered = Some(c.advert.clone()),
                    Err(err) => {
                        let msg = format!("no se pudo anunciar este equipo: {err}");
                        disc.with(|inner| inner.error = Some(msg));
                    }
                }
            }
            if disc.rescan.swap(false, Ordering::Relaxed) {
                let _ = d.stop_browse(SERVICE_TYPE);
                receiver = d.browse(SERVICE_TYPE).ok();
            }
        }

        match &receiver {
            Some(rx) => match rx.recv_timeout(Duration::from_secs(1)) {
                Ok(event) => {
                    handle_event(disc, event, &c.advert.id, c.notify);
                    while let Ok(event) = rx.try_recv() {
                        handle_event(disc, event, &c.advert.id, c.notify);
                    }
                }
                Err(mdns_sd::RecvTimeoutError::Timeout) => {}
                Err(mdns_sd::RecvTimeoutError::Disconnected) => {
                    receiver = daemon.as_ref().and_then(|d| d.browse(SERVICE_TYPE).ok());
                    std::thread::sleep(Duration::from_secs(1));
                }
            },
            None => std::thread::sleep(Duration::from_secs(1)),
        }

        if c.probe && last_probe.is_none_or(|t| t.elapsed() >= PROBE_EVERY) {
            last_probe = Some(Instant::now());
            let results: Vec<(SocketAddr, Probe)> =
                c.targets.iter().map(|addr| (*addr, probe(*addr))).collect();
            disc.with(|inner| {
                inner.probes.retain(|addr, _| c.targets.contains(addr));
                inner.probes.extend(results);
            });
        }

        // mDNS tarda mucho en dar por perdido a un equipo que se apagó sin
        // despedirse: se le pregunta de forma activa.
        if let Some(d) = &daemon {
            if last_verify.elapsed() >= VERIFY_EVERY {
                last_verify = Instant::now();
                let names: Vec<String> = disc
                    .with(|inner| {
                        inner
                            .peers
                            .values()
                            .filter(|p| p.online)
                            .map(|p| p.fullname.clone())
                            .collect()
                    })
                    .unwrap_or_default();
                for name in names {
                    let _ = d.verify(name, Duration::from_secs(3));
                }
            }
        }
    }

    if let Some(d) = daemon {
        if let Some(advert) = registered {
            if let Ok(done) = d.unregister(&fullname(&advert.id)) {
                let _ = done.recv_timeout(Duration::from_secs(1));
            }
        }
        let _ = d.shutdown();
    }
    disc.with(|inner| inner.running = false);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Advert {
        Advert {
            id: "equipo-47800".to_string(),
            name: "PC de casa".to_string(),
            version: "0.1.0".to_string(),
            os: "windows".to_string(),
            gpu: "AMD Radeon RX 6600".to_string(),
            vram_bytes: 8_573_157_376,
            memory: "dedicated".to_string(),
            backend: "vulkan".to_string(),
            rpc: Some("192.168.1.10:50052".parse().unwrap()),
            chat: Some("192.168.1.10:8080".parse().unwrap()),
            model: "qwen2.5:7b".to_string(),
        }
    }

    fn roundtrip(advert: &Advert) -> Option<Advert> {
        let txt = advert.to_txt();
        Advert::from_txt(|key| txt.iter().find(|(k, _)| *k == key).map(|(_, v)| v.clone()))
    }

    #[test]
    fn advert_roundtrip() {
        let advert = sample();
        assert_eq!(roundtrip(&advert), Some(advert));
        let quiet = Advert {
            rpc: None,
            model: String::new(),
            ..sample()
        };
        assert_eq!(roundtrip(&quiet), Some(quiet));
    }

    #[test]
    fn advert_rejects_foreign_or_broken() {
        assert_eq!(Advert::from_txt(|_| None), None);
        assert_eq!(
            Advert::from_txt(|k| (k == "proto").then(|| "1".to_string())),
            None
        );
        assert_eq!(
            Advert::from_txt(|k| match k {
                "proto" => Some("99".to_string()),
                "id" => Some("x".to_string()),
                _ => None,
            }),
            None
        );
        // Un valor basura no rompe nada: se ignora.
        let odd = Advert::from_txt(|k| match k {
            "proto" => Some("1".to_string()),
            "id" => Some("x".to_string()),
            "vram" => Some("muchísima".to_string()),
            "rpc" => Some("no-es-una-ip".to_string()),
            "name" => Some("n".repeat(5000)),
            _ => None,
        })
        .unwrap();
        assert_eq!(odd.vram_bytes, 0);
        assert_eq!(odd.rpc, None);
        // Una dirección pública anunciada se descarta.
        let public = Advert::from_txt(|k| match k {
            "proto" => Some("1".to_string()),
            "id" => Some("x".to_string()),
            "rpc" | "chat" => Some("8.8.8.8:80".to_string()),
            _ => None,
        })
        .unwrap();
        assert_eq!((public.rpc, public.chat), (None, None));
        assert_eq!(odd.name.chars().count(), MAX_FIELD_CHARS);
    }

    #[test]
    fn txt_values_fit_in_a_record() {
        let long = Advert {
            name: "ñ".repeat(500),
            gpu: "g".repeat(500),
            ..sample()
        };
        for (key, value) in long.to_txt() {
            assert!(key.len() + value.len() < 255, "{key}");
        }
    }

    #[test]
    fn ids() {
        assert_eq!(node_id("EQUIPO", 47800), "equipo-47800");
        assert_eq!(node_id("Mac de Raúl.local", 1), "mac-de-ra-l-local-1");
        assert_eq!(node_id("", 5), "nodo-5");
    }

    #[test]
    fn virtual_interfaces() {
        assert!(is_virtual_name("vEthernet (WSL (Hyper-V firewall))"));
        assert!(is_virtual_name("docker0"));
        assert!(!is_virtual_name("Wi-Fi"));
        assert!(!is_virtual_name("Ethernet"));
        assert!(!is_virtual_name("en0"));
    }

    #[test]
    fn prefers_address_on_our_subnet() {
        let local = [LocalAddr {
            name: "Ethernet".to_string(),
            ip: "192.168.1.10".parse().unwrap(),
            netmask: "255.255.255.0".parse().unwrap(),
        }];
        let ips: Vec<IpAddr> = ["172.20.0.1", "192.168.1.20", "127.0.0.1"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(
            pick_addr(&ips, &local),
            Some("192.168.1.20".parse().unwrap())
        );
        assert_eq!(pick_addr(&ips, &[]), Some("172.20.0.1".parse().unwrap()));
        let only_loop: Vec<IpAddr> = vec!["127.0.0.1".parse().unwrap()];
        assert_eq!(
            pick_addr(&only_loop, &local),
            Some("127.0.0.1".parse().unwrap())
        );
        assert_eq!(pick_addr(&[], &local), None);
    }

    #[test]
    fn preferred_ip_is_cable_then_wifi() {
        let iface = |name: &str, ip: &str| LocalAddr {
            name: name.to_string(),
            ip: ip.parse().unwrap(),
            netmask: "255.255.255.0".parse().unwrap(),
        };
        let wifi = iface("Wi-Fi", "192.168.1.65");
        let cable = iface("Ethernet", "169.254.10.1");
        let lan_cable = iface("Ethernet 2", "192.168.50.2");
        assert_eq!(
            preferred_lan_ip(&[wifi.clone(), cable.clone()]),
            Some("169.254.10.1".parse().unwrap())
        );
        // Una red con DHCP gana a la dirección de enlace local.
        assert_eq!(
            preferred_lan_ip(&[wifi.clone(), cable, lan_cable]),
            Some("192.168.50.2".parse().unwrap())
        );
        assert_eq!(
            preferred_lan_ip(&[wifi]),
            Some("192.168.1.65".parse().unwrap())
        );
        assert_eq!(preferred_lan_ip(&[]), None);
    }

    #[test]
    fn prefers_cable_over_wifi() {
        let iface = |name: &str, ip: &str, mask: &str| LocalAddr {
            name: name.to_string(),
            ip: ip.parse().unwrap(),
            netmask: mask.parse().unwrap(),
        };
        let local = [
            iface("Wi-Fi", "10.0.0.5", "255.255.255.0"),
            iface("Ethernet", "169.254.10.1", "255.255.0.0"),
        ];
        // El vecino es alcanzable por las dos redes; 10.0.0.9 ordena antes.
        let ips: Vec<IpAddr> = ["10.0.0.9", "169.254.77.3"]
            .iter()
            .map(|s| s.parse().unwrap())
            .collect();
        assert_eq!(
            pick_addr(&ips, &local),
            Some("169.254.77.3".parse().unwrap())
        );
        assert_eq!(
            pick_addr(&ips, &local[..1]),
            Some("10.0.0.9".parse().unwrap())
        );
        assert!(is_wifi_name("Wi-Fi") && !is_wifi_name("Ethernet 2"));
    }

    #[test]
    fn join_leave_and_rpc_changes() {
        let mut inner = Inner::default();
        let quiet = Advert {
            rpc: None,
            ..sample()
        };
        let apply = |inner: &mut Inner, advert: &Advert| {
            apply_resolved(inner, advert.clone(), "n".to_string(), None)
        };
        assert_eq!(apply(&mut inner, &quiet), [Change::Joined]);
        assert_eq!(apply(&mut inner, &quiet), []);
        assert_eq!(apply(&mut inner, &sample()), [Change::RpcUp]);
        assert_eq!(apply(&mut inner, &quiet), [Change::RpcDown]);
        inner.peers.get_mut(&quiet.id).unwrap().online = false;
        // Al volver compartiendo GPU es una sola novedad: se conectó.
        assert_eq!(apply(&mut inner, &sample()), [Change::Joined]);
    }

    #[test]
    fn probe_reports_closed_port() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(probe(addr).reachable);
        drop(listener);
        assert!(!probe(addr).reachable);
    }
}
