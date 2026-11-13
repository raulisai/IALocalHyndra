# Matriz de hardware

Fuente: `uac-agent probe`. Un JSON por nodo en `docs/hardware/`, que **no se versiona** (incluye nombre del equipo y rutas del usuario). Esta tabla es el resumen que sí se comparte. Es un inventario, no una prueba de cómputo: ningún backend pasa a `verified` hasta que un benchmark calcule tensores (T0.6).

## Nodos

| | AMD (este) | RTX | MAC |
|---|---|---|---|
| Informe (local) | `amd-rx6600.json` | `rtx-3070-laptop.json` | pendiente (T0.2) |
| SO | Windows 11 Pro 10.0.26300 | Windows 11 Pro 10.0.26300 | ? |
| CPU | Ryzen 5 3400G, 4 núcleos / 8 hilos | i7-11800H, 8 núcleos / 16 hilos (portátil) | Apple M4 |
| RAM | 31,9 GiB | 31,7 GiB | ? |
| GPU | Radeon RX 6600 | RTX 3070 **Laptop** (cc 8.6) + Intel UHD integrada | Apple M4 (unificada) |
| Memoria GPU | 8,0 GiB | 8,0 GiB (nvidia-smi) | ? |
| Utilizable por el planner | 6,8 GiB | 6,8 GiB | ? |
| Backends | Vulkan `detected` · ROCm `unsupported` | CUDA `detected` (probe) y ejercitado con inferencia correcta · Vulkan `detected` | Metal `expected` |
| Red | Wi-Fi 6, ~300 Mb/s de enlace | Wi-Fi 6E (Killer AX1675x), 867 Mb/s de enlace | ? |
| llama.cpp `b12110` | instalado; Vulkan ejercitado con inferencia correcta ([resultados](benchmarks/llama-bench.md)) | instalado (CUDA 12.4); CUDA ejercitado con inferencia correcta | pendiente |
| Falta instalar | iperf3 | iperf3, git | ? |

### Puesta en marcha del nodo RTX (2026-11-13)

El disco del sistema tiene ~3 GB libres, así que todo se instaló en `D:\uac\` (fuera del repo):

| Carpeta | Contenido |
|---|---|
| `D:\uac\rust` | rustup y cargo (`RUSTUP_HOME`, `CARGO_HOME`), toolchain `stable-x86_64-pc-windows-gnu` |
| `D:\uac\mingw` | MinGW-w64 (WinLibs): el `dlltool` de Rust GNU necesita su `as.exe` para compilar los crates `windows-*` |
| `D:\uac\target` | `CARGO_TARGET_DIR` |
| `D:\uac\llama` | llama.cpp `b12110` CUDA 12.4 + runtime cudart |
| `D:\uac\models` | `qwen2.5-3b-q4_k_m.gguf` (mismo hash que en AMD) |

Se usa el toolchain GNU porque MSVC exige Visual Studio Build Tools (~6 GB en C:). Variables para compilar en una consola nueva:

```powershell
$env:RUSTUP_HOME='D:\uac\rust\rustup'; $env:CARGO_HOME='D:\uac\rust\cargo'; $env:CARGO_TARGET_DIR='D:\uac\target'; $env:Path="D:\uac\rust\cargo\bin;D:\uac\mingw\mingw64\bin;$env:Path"
```

Al arrancar `llama-server` por primera vez, Windows creó reglas de entrada que valen para todos los puertos, cualquier IP y también para redes **públicas**. Para dejarlas limitadas a la LAN (en PowerShell como administrador):

```powershell
Get-NetFirewallRule -DisplayName llama-server | Remove-NetFirewallRule
New-NetFirewallRule -DisplayName 'UAC llama-server (LAN)' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 8080 -RemoteAddress LocalSubnet -Profile Private -Program 'D:\uac\llama\llama-server.exe'
New-NetFirewallRule -DisplayName 'UAC ggml-rpc-server (pares)' -Direction Inbound -Action Allow -Protocol TCP -LocalPort 50052 -RemoteAddress <IP-AMD>,<IP-MAC> -Profile Private -Program 'D:\uac\llama\ggml-rpc-server.exe'
```

`%APPDATA%\uac\config.json` apunta `llama_cpp_dir` a `D:\uac\llama` y `model_dirs` a `D:\uac\models`. Para prestar la GPU por RPC, poner `rpc.device` en `CUDA0` (si se deja vacío también ofrece la Intel UHD por Vulkan).

«Utilizable» aplica la política por defecto de `uac-planner`: se reserva el 15 % en VRAM dedicada y el 35 % en memoria unificada, con un mínimo de 1 GiB. Son valores iniciales sin calibrar.

### Descubrimiento y conexión (2026-11-13)

Con la versión de GitHub (830d315, mDNS, ADR 0006) este nodo encontró solo al nodo AMD (DJOKER, 192.168.1.65, RX 6600 Vulkan) en la Wi-Fi, sin escribir IP. Una segunda instancia aislada en este equipo encontró al nodo RTX con su RPC verificado (2 ms) y su chat compartido; el chat compartido responde 401 sin clave y contesta con ella. **Falta**: que el nodo AMD comparta su GPU (RPC) para repartir un modelo entre los dos; hasta entonces no hay prueba de tensores entre máquinas.

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

