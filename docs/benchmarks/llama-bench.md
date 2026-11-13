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

## Nodo RTX — RTX 3070 Laptop, CUDA (2026-11-13)

Portátil con i7-11800H. Windows 11, driver NVIDIA 617.14, binarios `win-cuda-12.4` del build `b12110` (hash comprobado). llama.cpp ve `CUDA0: NVIDIA GeForce RTX 3070 Laptop GPU (8191 MiB, 7128 MiB free)`, compute capability 8.6. Mismo GGUF que el nodo AMD (SHA-256 comprobado).

| Modelo | Modo | pp512 | tg128 |
|---|---|---:|---:|
| qwen2.5 3B Q4_K_M | CUDA directo | 5339,85 ± 457,87 | 138,39 ± 0,47 |

Prueba del prototipo (P.3): `uac-node` arrancó `llama-server` con este modelo y `/v1/chat/completions` respondió a 111 tok/s, con 2,4 GiB de VRAM ocupada.

### Notas

- Es la versión **Laptop** de la 3070: su límite de potencia es menor que el de la de escritorio, así que las cifras no valen para una 3070 de escritorio.
- La desviación de `pp512` es alta (±9 %); probablemente por la gestión de energía del portátil. Conviene repetir enchufado y en modo de máximo rendimiento.
- Frente a la RX 6600 con el mismo modelo: generación ×1,6 (138 frente a 86 tok/s), prompt ×5,2.
- Falta el 7B: no se descargó por falta de espacio en disco (ver [hardware-matrix.md](../hardware-matrix.md)).

## Nodo MAC — M4, Metal

Pendiente (T0.7).

## RPC entre nodos

Pendiente (T0.8, T0.9).
