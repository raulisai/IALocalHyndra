# Matriz de hardware

Fuente: `uac-agent probe`. Un JSON por nodo en [hardware/](hardware/). Es un inventario, no una prueba de cómputo: ningún backend pasa a `verified` hasta que un benchmark calcule tensores (T0.6).

## Nodos

| | AMD (este) | RTX | MAC |
|---|---|---|---|
| Informe | [amd-rx6600.json](hardware/amd-rx6600.json) | pendiente (T0.2) | pendiente (T0.2) |
| SO | Windows 11 Pro 10.0.26300 | ? | ? |
| CPU | Ryzen 5 3400G, 4 núcleos / 8 hilos | ? | Apple M4 |
| RAM | 31,9 GiB | ? | ? |
| GPU | Radeon RX 6600 | RTX 3070 | Apple M4 (unificada) |
| Memoria GPU | 8,0 GiB | 8 GB nominales | ? |
| Utilizable por el planner | 6,8 GiB | ? | ? |
| Backends | Vulkan `detected` · ROCm `unsupported` | CUDA `expected` | Metal `expected` |
| Red | Wi-Fi 6, ~300 Mb/s de enlace | ? | ? |
| llama.cpp `b12110` | instalado; Vulkan ejercitado con inferencia correcta ([resultados](benchmarks/llama-bench.md)) | pendiente | pendiente |
| Falta instalar | iperf3 | ? | ? |

«Utilizable» aplica la política por defecto de `uac-planner`: se reserva el 15 % en VRAM dedicada y el 35 % en memoria unificada, con un mínimo de 1 GiB. Son valores iniciales sin calibrar.

## Red entre pares (T0.4)

| Par | Enlace | `iperf3` → | `iperf3` ← | Latencia |
|---|---|---|---|---|
| AMD ↔ RTX | ? | ? | ? | ? |
| AMD ↔ MAC | ? | ? | ? | ? |
| RTX ↔ MAC | ? | ? | ? | ? |

## Cómo añadir un nodo

En el nodo, desde la raíz del repo:

```bash
cargo run -p uac-agent -- probe --out docs/hardware/<nombre>.json
```

Después, en cualquier máquina:

```bash
cargo run -p uac-controller -- nodes
```

Las rutas de macOS y Linux del probe compilan y tienen tests de sus parsers, pero no se han ejecutado en un equipo real. Si algo sale vacío o con avisos, corregir `crates/uac-probe/src/macos.rs` o `linux.rs`.
