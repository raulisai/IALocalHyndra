# llama.cpp: versión fijada e instalación por nodo

Todos los nodos usan **el mismo build** de llama.cpp. El RPC es un protocolo interno sin garantías entre versiones, así que mezclar builds no está permitido.

## Versión fijada

| | |
|---|---|
| Release estable | `v0.9.0` (2026-11-04) |
| Build con binarios | `b12110` |
| Commit | `cd959bae99ff4bed22bef2604f45241c09bbb8c8` |
| Protocolo RPC | v8.0.1 (lo imprime `ggml-rpc-server` al arrancar) |

No hace falta compilar: los binarios oficiales incluyen el backend de cada plataforma, `ggml-rpc-server` y el cliente RPC.

Descarga: `https://github.com/ggml-org/llama.cpp/releases/tag/b12110`

| Nodo | Archivo | SHA-256 |
|---|---|---|
| AMD (Windows, Vulkan) | `llama-b12110-bin-win-vulkan-x64.zip` | `9732a5e48887ca54d0bfe4af4324dc85fd56cd8cda19b52df3542fa6ebe6874f` |
| MAC (Metal) | `llama-b12110-bin-macos-arm64.tar.gz` | `4f5b8de12e0b9516658ca6a5764c6ffb3d7c659c3dd538556b1413c7b4050662` |
| RTX (Windows, CUDA 12.4) | `llama-b12110-bin-win-cuda-12.4-x64.zip` | `1ce755bc3a061889234048f88e33328e17971eaad73c4666cfeb26026b317fee` |
| RTX, runtime CUDA 12.4 | `cudart-llama-bin-win-cuda-12.4-x64.zip` | `8c79a9b226de4b3cacfd1f83d24f962d0773be79f1e7b75c6af4ded7e32ae1d6` |
| RTX (Windows, CUDA 13.4) | `llama-b12110-bin-win-cuda-13.4-x64.zip` | `dc910a969190b5d42e189afd73117f6bbdd8dd8ed6586342c5e0b82cab62c673` |
| RTX, runtime CUDA 13.4 | `cudart-llama-bin-win-cuda-13.4-x64.zip` | `738f8c251ac22b70c3ae6f83a10cf222725df0395246a2cf58f32bdb85fbe668` |

Los hashes son los que publica GitHub para cada archivo del release. Solo el de Vulkan se ha descargado y comprobado. En el nodo RTX se elige 12.4 o 13.4 según el driver; si el nodo acaba en Linux, hay que compilar en ese mismo commit con `-DGGML_CUDA=ON -DGGML_RPC=ON`.

Los binarios se descomprimen en `tools/llama.cpp/b12110/`, que está fuera de git.

## Modelo de referencia

Mismo GGUF en todos los nodos, identificado por su hash:

| Modelo | Tamaño | SHA-256 |
|---|---|---|
| `qwen2.5:7b` de Ollama (Q4_K_M) | 4,36 GiB | `2bada8a7450677000f678be90653b85d364de7db25eb5ea54136ada5f3933730` |
| `qwen2.5:3b` de Ollama (Q4_K_M) | 1,79 GiB | `5ee4f07cdb9beadbbb293e85803c569b01bd37ed059d2715faa7bb405f31caa6` |

Con Ollama instalado, `ollama pull qwen2.5:7b` deja el archivo en `~/.ollama/models/blobs/sha256-<hash>`. Es un GGUF normal y llama.cpp lo abre directamente.

Para la prueba de reparto (T0.9) ya hay un modelo que no cabe en 8 GB en el nodo AMD: `Qwen3.8-27B-Q4_K_M.gguf` de LM Studio, 15,66 GB.

## Benchmark de un nodo (T0.7)

```bash
./llama-bench -m <ruta-al-gguf-7b> -ngl 99 -r 3 -o md
```

Copiar la tabla a [benchmarks/llama-bench.md](benchmarks/llama-bench.md).

## Prueba RPC entre dos nodos (T0.8)

En el nodo que presta su GPU:

```bash
./ggml-rpc-server -H <IP-LAN-de-este-nodo> -p 50052 -c
```

En el nodo anfitrión:

```bash
./llama-bench --rpc <IP-del-servidor>:50052 -dev RPC0 -m <ruta-al-gguf-7b> -ngl 99 -r 3 -o md
```

`-dev RPC0` obliga a usar solo la GPU remota. Sin esa opción, llama.cpp reparte entre la GPU local y la remota en proporción a su memoria.

**Seguridad.** `ggml-rpc-server` no tiene autenticación ni cifrado. Por defecto escucha solo en `127.0.0.1`. Al pasarle `-H` con la IP de la LAN hay que crear antes una regla de firewall que admita el puerto 50052 únicamente desde las IP de los otros dos nodos. Nunca `-H 0.0.0.0` en una red que no sea propia.

Para ver bytes y tiempos por comando, definir `GGML_RPC_DEBUG=3` en servidor o cliente; en el cliente añadir además `-lv 5`.
