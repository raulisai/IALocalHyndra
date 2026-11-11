# ADR 0001 — Alcance reducido y evidencia primero

**Estado:** propuesta · 2026-11-11

## Contexto

La investigación v1 evalúa 20 proyectos y planea 8 crates y 7 plugins. La mayoría de esos proyectos son de centro de datos Linux/NVIDIA y no corren en los tres nodos reales (Mac M4, RTX 3070, RX 6600 en Windows). La prueba más barata de viabilidad, repartir un LLM con llama.cpp RPC, estaba detrás de varias semanas de plano de control.

## Decisión

- Camino crítico con cuatro piezas: llama.cpp (inferencia repartida), Burn (cómputo entre fabricantes), PyTorch (jobs completos por nodo), iroh (red).
- Ray, dstack, Spark, Dynamo, vLLM, DeepSpeed, Megatron, UCX, MAX, IREE, TVM Disco y ONNX Runtime quedan como bibliografía. Vuelven solo con una necesidad concreta y un nodo donde corran.
- Fase 0.5 nueva: baseline manual de llama.cpp RPC antes de escribir el plano de control.
- Los criterios de abandono se fijan antes de medir (ver arquitectura v2, sección 7).

## Consecuencias

- Menos superficie y resultados medidos en días.
- Se renuncia a comparar compiladores (IREE/TVM) por ahora.
- Si Burn falla en la Fase 3, no hay segundo runtime preparado: habrá que reabrir esa decisión.
