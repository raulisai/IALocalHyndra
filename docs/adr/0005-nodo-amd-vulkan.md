# ADR 0005 — El nodo AMD usa Vulkan y aloja el controller

**Estado:** propuesta · 2026-11-11

## Contexto

El probe identificó el nodo AMD: Radeon RX 6600 (RDNA2, 8 GB), Ryzen 5 3400G, 32 GB RAM, Windows 11, cargador Vulkan presente, WSL instalado, red por Wi-Fi 6 con enlace negociado de unos 300 Mb/s (292–325 en dos lecturas).

La serie RX 6000 de consumo no aparece en las matrices de compatibilidad ROCm de AMD: ni en la 7.2.1 para Radeon (Windows y Linux) ni en la 10.1.0 unificada. Existen builds comunitarios de ROCm para RDNA2 con `HSA_OVERRIDE_GFX_VERSION`, sin pruebas publicadas en la RX 6600. El soporte de PyTorch en ZLUDA estaba en desarrollo a finales de 2025.

## Decisión

- Backend del nodo: **Vulkan**. Motores: llama.cpp, stable-diffusion.cpp y Burn.
- No se planifica ningún job PyTorch-GPU en este nodo.
- El nodo aloja el controller y el almacén de artefactos: tiene 32 GB de RAM y es el que menos cómputo GPU aporta.
- Cablear Ethernet antes de incluirlo en cualquier job repartido.

## Evidencia (2026-11-11)

llama.cpp `b12110` corre en la RX 6600 con Vulkan: 23,9 tok/s de generación en un modelo 7B Q4_K_M, frente a 3,6 en CPU. El mismo nodo sirve como `ggml-rpc-server` en loopback con salida idéntica a la local. Detalle en [benchmarks/llama-bench.md](../benchmarks/llama-bench.md).

## Consecuencias

- Los pipelines PyTorch (Diffusers, 3D) solo se reparten entre RTX y Mac.
- Vulkan suele rendir menos que ROCm o CUDA en el mismo hardware; hay que medirlo.
- Probar ROCm comunitario o ZLUDA queda como experimento opcional, fuera del camino crítico.
