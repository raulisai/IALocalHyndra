//! Presupuestos de memoria, ubicación de cargas y coste de red.
//!
//! Lógica pura, sin E/S. Los valores por defecto de [`MemoryPolicy`] son un
//! punto de partida sin calibrar: se ajustan con mediciones reales (T0.6).

use uac_protocol::{MemoryKind, NodeReport};

pub const GIB: u64 = 1 << 30;

/// Cuánta memoria se deja libre en cada dispositivo. Nunca se planifica al 100 %.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MemoryPolicy {
    /// Fracción reservada en GPU con VRAM propia (driver, escritorio, staging).
    pub dedicated_reserve_fraction: f64,
    /// Fracción reservada en memoria unificada: el sistema la comparte.
    pub unified_reserve_fraction: f64,
    /// Reserva mínima absoluta por dispositivo.
    pub min_reserve_bytes: u64,
}

impl Default for MemoryPolicy {
    fn default() -> Self {
        Self {
            dedicated_reserve_fraction: 0.15,
            unified_reserve_fraction: 0.35,
            min_reserve_bytes: GIB,
        }
    }
}

/// Memoria planificable de un dispositivo. Las GPU integradas dan cero.
pub fn usable_bytes(total: u64, kind: MemoryKind, policy: &MemoryPolicy) -> u64 {
    let fraction = match kind {
        MemoryKind::Dedicated => policy.dedicated_reserve_fraction,
        MemoryKind::Unified => policy.unified_reserve_fraction,
        MemoryKind::Shared => return 0,
    };
    let reserve = ((total as f64 * fraction) as u64).max(policy.min_reserve_bytes);
    total.saturating_sub(reserve)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub node: String,
    pub gpu: String,
    pub total_bytes: u64,
    pub usable_bytes: u64,
}

/// Dispositivos planificables de un conjunto de informes. Omite las GPU sin
/// memoria conocida y las integradas.
pub fn devices(reports: &[NodeReport], policy: &MemoryPolicy) -> Vec<Device> {
    reports
        .iter()
        .flat_map(|report| {
            report.gpus.iter().filter_map(move |gpu| {
                let total = gpu.vram_bytes?;
                let usable = usable_bytes(total, gpu.memory_kind, policy);
                (usable > 0).then(|| Device {
                    node: report.hostname.clone(),
                    gpu: gpu.name.clone(),
                    total_bytes: total,
                    usable_bytes: usable,
                })
            })
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Share {
    /// Índice en la lista de dispositivos pasada a [`place`].
    pub device: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    /// Cabe entero en un dispositivo: no se reparte.
    SingleNode {
        device: usize,
    },
    /// Solo cabe repartido, en proporción a la memoria de cada dispositivo.
    Split {
        shares: Vec<Share>,
    },
    DoesNotFit {
        deficit_bytes: u64,
    },
}

/// Decide dónde ubicar una carga que necesita `required` bytes.
///
/// - Si cabe en algún dispositivo, elige el más pequeño donde quepa y deja
///   libres los grandes.
/// - Si no, reparte entre el menor número de dispositivos posible (menos saltos
///   de red), en proporción a su memoria utilizable.
pub fn place(required: u64, devices: &[Device]) -> Placement {
    let single = devices
        .iter()
        .enumerate()
        .filter(|(_, d)| d.usable_bytes >= required)
        .min_by_key(|(_, d)| d.usable_bytes);
    if let Some((device, _)) = single {
        return Placement::SingleNode { device };
    }

    let mut order: Vec<usize> = (0..devices.len()).collect();
    order.sort_by_key(|&i| std::cmp::Reverse(devices[i].usable_bytes));

    let mut chosen = Vec::new();
    let mut capacity: u64 = 0;
    for i in order {
        chosen.push(i);
        capacity += devices[i].usable_bytes;
        if capacity >= required {
            break;
        }
    }
    if capacity < required {
        return Placement::DoesNotFit {
            deficit_bytes: required - capacity,
        };
    }

    let mut shares = Vec::with_capacity(chosen.len());
    let mut assigned: u64 = 0;
    for (pos, &device) in chosen.iter().enumerate() {
        let bytes = if pos + 1 == chosen.len() {
            required - assigned
        } else {
            (required as u128 * devices[device].usable_bytes as u128 / capacity as u128) as u64
        };
        assigned += bytes;
        shares.push(Share { device, bytes });
    }
    Placement::Split { shares }
}

/// Segundos mínimos para mover `bytes` por un enlace de `link_bps` bits por
/// segundo. Es una cota inferior: ignora latencia, overhead y contención.
pub fn transfer_seconds(bytes: u64, link_bps: u64) -> Option<f64> {
    (link_bps > 0).then(|| bytes as f64 * 8.0 / link_bps as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn device(node: &str, usable_gib: f64) -> Device {
        Device {
            node: node.to_string(),
            gpu: "gpu".to_string(),
            total_bytes: 0,
            usable_bytes: (usable_gib * GIB as f64) as u64,
        }
    }

    #[test]
    fn reserves_memory() {
        let policy = MemoryPolicy::default();
        // 8 GiB dedicados: 15 % son 1,2 GiB, por encima del mínimo de 1 GiB.
        let usable = usable_bytes(8 * GIB, MemoryKind::Dedicated, &policy);
        assert_eq!(usable, 8 * GIB - (8.0 * GIB as f64 * 0.15) as u64);
        // 4 GiB dedicados: 15 % son 0,6 GiB, se aplica el mínimo de 1 GiB.
        assert_eq!(
            usable_bytes(4 * GIB, MemoryKind::Dedicated, &policy),
            3 * GIB
        );
        assert_eq!(usable_bytes(2 * GIB, MemoryKind::Shared, &policy), 0);
        assert_eq!(usable_bytes(GIB / 2, MemoryKind::Dedicated, &policy), 0);
    }

    #[test]
    fn prefers_smallest_single_device() {
        let devs = [device("mac", 15.6), device("rtx", 6.8), device("amd", 6.8)];
        assert_eq!(place(5 * GIB, &devs), Placement::SingleNode { device: 1 });
        assert_eq!(place(12 * GIB, &devs), Placement::SingleNode { device: 0 });
    }

    #[test]
    fn splits_across_fewest_devices() {
        let devs = [device("rtx", 6.8), device("amd", 6.8), device("mac", 15.6)];
        let Placement::Split { shares } = place(20 * GIB, &devs) else {
            panic!("debía repartirse");
        };
        // Mac + una GPU bastan (22,4 GiB): no se usa el tercer nodo.
        assert_eq!(shares.len(), 2);
        assert_eq!(shares[0].device, 2);
        assert_eq!(shares.iter().map(|s| s.bytes).sum::<u64>(), 20 * GIB);
        for share in &shares {
            assert!(share.bytes <= devs[share.device].usable_bytes);
        }
    }

    #[test]
    fn reports_deficit() {
        let devs = [device("rtx", 6.0), device("amd", 6.0)];
        assert_eq!(
            place(20 * GIB, &devs),
            Placement::DoesNotFit {
                deficit_bytes: 8 * GIB
            }
        );
        assert_eq!(
            place(GIB, &[]),
            Placement::DoesNotFit { deficit_bytes: GIB }
        );
    }

    #[test]
    fn transfer_lower_bound() {
        // 4 GB por Ethernet de 1 Gb/s: 32 s, como en la tabla de la investigación.
        assert_eq!(transfer_seconds(4_000_000_000, 1_000_000_000), Some(32.0));
        assert_eq!(transfer_seconds(1, 0), None);
    }
}
