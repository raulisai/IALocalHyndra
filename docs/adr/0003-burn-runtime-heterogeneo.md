# ADR 0003 — Burn como runtime entre fabricantes; PyTorch para jobs completos

**Estado:** propuesta · 2026-11-11

## Contexto

v1 pone PyTorch como motor de entrenamiento de referencia, Burn como experimento y un crate propio `uac-tensor`. Pero:

- PyTorch no tiene GPU oficial en la RX 6600 (fuera de la matriz ROCm 7.2.1 en Windows y Linux).
- `torch.distributed` no funciona sobre MPS.
- Burn cubre CUDA, Metal y Vulkan con una sola API y autodiff. La 0.22 (oct. 2026) añade backend remoto sobre iroh o WebSocket, remoto multi-dispositivo, `DistributedContext` + `all_reduce` y transferencias entre backends que preservan el grafo. La 0.23-pre añade `burn-shard` y entrenamiento en pipeline (GPipe, 1F1B).

## Decisión

- Burn es el runtime para todo cómputo que cruce fabricantes (Fase 3 y código nuevo).
- No se crea `uac-tensor` ni collectives propios.
- PyTorch queda como motor de jobs completos en un solo nodo (RTX, Mac, AWS), dentro de un sidecar Python gestionado con uv.
- Se fija una versión exacta de Burn en todo el clúster: cliente y servidor remotos deben actualizarse juntos (versión de protocolo).

## Consecuencias

- Los modelos PyTorch existentes no corren repartidos sin portarlos a Burn. Los pesos sí se importan (safetensors / PyTorch vía `burn-store`); la importación ONNX cubre un conjunto limitado de operadores.
- Burn avisa de cambios incompatibles frecuentes. El coste de actualizar es real.
- Lo que falte se contribuye upstream en lugar de bifurcar.

## Por verificar (Fase 3)

- Gradientes de un modelo partido entre dos backends frente a referencia local.
- `all_reduce` entre nodos remotos de backends distintos: no está documentado.
- Rendimiento de Burn sobre Vulkan en la RX 6600.
