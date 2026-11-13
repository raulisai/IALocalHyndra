# Montar un nodo nuevo

Guía para llevar el proyecto a otro equipo (Mac M4 o PC con RTX) y completar T0.2, T0.7 y T0.8.

## 1. Llevar el repositorio

El repo no tiene remoto. En el equipo de origen se genera un paquete con todo el historial:

```bash
git bundle create uac.bundle --all
```

Se copia `uac.bundle` al otro equipo (USB, red local) y allí:

```bash
git clone uac.bundle universal-ai-cluster
```

## 2. Requisitos

- Rust 1.85 o posterior: https://rustup.rs
- En Windows, Visual Studio Build Tools con «Desarrollo para el escritorio con C++».
- En Mac: `xcode-select --install`.

Comprobar que todo compila y pasa:

```bash
cargo test --workspace
```

## 3. Inventario del nodo (T0.2)

```bash
cargo run -p uac-agent -- probe --out docs/hardware/<nombre>.json
```

El JSON no se versiona (contiene el nombre del equipo). Revisa la salida: **las rutas de macOS y Linux nunca se han ejecutado en un equipo real**. Si faltan datos o salen avisos, anótalos o corrige `crates/uac-probe/src/macos.rs` o `linux.rs`.

## 4. llama.cpp (T0.7)

Mismo build en todos los nodos: `b12110`, commit `cd959bae9`. Archivos y SHA-256 en [llama-cpp.md](llama-cpp.md). Descomprimir en `tools/llama.cpp/b12110/`.

Verificar el hash antes de descomprimir:

```bash
shasum -a 256 llama-b12110-bin-macos-arm64.tar.gz
```

En Windows: `Get-FileHash <archivo> -Algorithm SHA256`.

Comprobar que el build incluye lo necesario (no se ha verificado para macOS ni CUDA):

```bash
./llama-bench --list-devices
```

Debe listar la GPU (`Metal` o `CUDA0`). Si falta `ggml-rpc-server`, ese build no sirve para T0.8 y habrá que compilar con `-DGGML_RPC=ON` en el mismo commit.

Benchmark con el mismo modelo que en el nodo AMD (qwen2.5 7B Q4_K_M, hash en [llama-cpp.md](llama-cpp.md)):

```bash
./llama-bench -m <ruta-al-gguf> -ngl 99 -r 3 -o md
```

Añade la tabla a [benchmarks/llama-bench.md](benchmarks/llama-bench.md).

## 5. Panel del nodo

```bash
cargo run -p uac-agent --bin uac-node
```

Abre `http://127.0.0.1:47800/`. El icono de bandeja solo existe en Windows; en Mac y Linux el programa corre sin icono y se sale con Ctrl+C. **El panel no se ha probado fuera de Windows.**

## 6. Prueba RPC entre equipos (T0.8)

Necesitas la IP de LAN de cada equipo y ambos en la misma red.

1. En el equipo que presta su GPU, crea una regla de firewall que admita el puerto 50052 **solo desde la IP del otro equipo**. El servidor no tiene autenticación.
2. Arráncalo con `-H <su-IP-de-LAN>` (desde el panel: IP de escucha en la configuración; pedirá confirmación).
3. En el otro equipo, anfitrión:

```bash
./llama-bench --rpc <IP-del-servidor>:50052 -dev RPC0 -m <gguf> -ngl 99 -r 3 -o md
```

4. Para comprobar que el resultado es correcto, genera el mismo texto con temperatura 0 y semilla fija en local y por RPC, y compara (como se hizo en loopback).

Mide también la red entre ambos con `iperf3` antes de interpretar los tok/s, y anota si es Wi-Fi o Ethernet.
