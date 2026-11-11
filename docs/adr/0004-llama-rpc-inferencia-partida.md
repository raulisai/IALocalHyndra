# ADR 0004 — llama.cpp RPC para la inferencia repartida de LLM

**Estado:** propuesta · 2026-11-11

## Contexto

Las alternativas para repartir un LLM entre Metal, CUDA y Vulkan:

| Opción | Situación |
|---|---|
| llama.cpp RPC | Funciona hoy entre backends distintos; caché de tensores, reparto proporcional, `--tensor-split`. Declarado frágil e inseguro por sus autores |
| exo | Solo MLX; Linux en CPU; Windows ausente; Mac y Linux no se unen |
| prima.cpp | Planificador heterogéneo sobre llama.cpp (ICLR 2026). Soporte Windows/Vulkan sin confirmar |
| GPUStack | Producto completo de gestión; reparto heterogéneo vía llama.cpp |
| Pipeline de capas propio | Semanas de trabajo para alcanzar lo que llama.cpp ya hace |

## Decisión

- llama.cpp RPC es el motor de reparto de LLM. Se usa primero a mano (Fase 0.5) y después supervisado por el agente (Fase 2).
- Mismo commit de llama.cpp en todos los nodos, fijado en el repo.
- `rpc-server` nunca se expone fuera de la LAN: firewall restringido a las IP de los pares. Túnel por iroh solo si se mide que compensa.
- Regla del planner: si el modelo cabe en un nodo, no se reparte.
- prima.cpp y GPUStack se evalúan en T2.4 con los números propios como referencia.

## Consecuencias

- Solo modelos GGUF.
- Se hereda la fragilidad del RPC: sin tolerancia a la caída de un nodo a mitad de generación.
- No se escribe un pipeline de capas propio salvo que T2.4 demuestre que hace falta.

## Por verificar

- `rpc-server` con backend Vulkan frente a anfitrión Metal y CUDA (T0.8).
