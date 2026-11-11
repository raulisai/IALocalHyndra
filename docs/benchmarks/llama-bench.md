# Resultados de llama-bench

Build `b12110` (commit `cd959bae9`) en todos los nodos. Versión, modelos y comandos en [../llama-cpp.md](../llama-cpp.md).

`pp512` es procesamiento de un prompt de 512 tokens; `tg128` es generación de 128 tokens. Unidades: tokens por segundo, media ± desviación de 3 repeticiones salvo que se indique.

## Nodo AMD — Radeon RX 6600, Vulkan (2026-11-11)

Windows 11, driver propietario AMD 32.0.21045.5002, Vulkan 1.4.315. llama.cpp ve `Vulkan0: AMD Radeon RX 6600 (8176 MiB, 7378 MiB free)`.

| Modelo | Modo | pp512 | tg128 |
|---|---|---:|---:|
| qwen2.5 3B Q4_K_M | Vulkan directo | 1023,04 ± 1,21 | 85,91 ± 0,08 |
| qwen2.5 3B Q4_K_M | RPC en loopback → Vulkan | 962,74 ± 11,81 | 71,18 ± 1,00 |
| qwen2.5 7B Q4_K_M | Vulkan directo | 481,21 ± 2,17 | 23,93 ± 0,01 |
| qwen2.5 7B Q4_K_M | RPC en loopback → Vulkan | 459,91 ± 5,33 | 22,38 ± 0,14 |
| qwen2.5 7B Q4_K_M | `-ngl 0` (1 repetición) | 236,67 | 3,62 |

### Lo que demuestran

- **Vulkan funciona como backend de `ggml-rpc-server`.** Era uno de los puntos sin verificar de la arquitectura. Cliente y servidor estaban en la misma máquina: falta repetirlo con un anfitrión Metal o CUDA (T0.8).
- **La salida por RPC es idéntica a la local.** El mismo prompt con temperatura 0 y semilla fija en el modelo 3B produjo 316 bytes iguales byte a byte por las dos rutas.
- **Coste del RPC sin red de por medio:** generación −17 % en 3B y −6 % en 7B; prompt −6 % y −4 %. Es el techo de lo que puede rendir este nodo como servidor remoto; por la red real será menos.
- **La GPU multiplica por 6,6 la generación frente a la CPU** en el modelo 7B (23,93 frente a 3,62 tok/s).

### Dudas abiertas

- El 7B genera 3,6 veces más lento que el 3B con solo 2,5 veces más parámetros. No se investigó la causa.
- El `pp512` de la fila `-ngl 0` no es una cifra de CPU pura: es demasiado alto para este procesador, así que llama.cpp probablemente siguió usando la GPU para el lote del prompt. La cifra de CPU fiable es `tg128`.
- Wi-Fi no intervino en ninguna medición.

## Nodo RTX — RTX 3070, CUDA

Pendiente (T0.7).

## Nodo MAC — M4, Metal

Pendiente (T0.7).

## RPC entre nodos

Pendiente (T0.8, T0.9).
