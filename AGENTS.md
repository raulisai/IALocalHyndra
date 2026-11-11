# Guía para agentes de código

## Antes de empezar

1. Leer [docs/arquitectura-v2.md](docs/arquitectura-v2.md) y los ADR de [docs/adr/](docs/adr/).
2. Tomar una tarea de [docs/backlog.md](docs/backlog.md) respetando dependencias y marcar su estado al terminar.

## Reglas del proyecto

- **Nada se declara compatible sin medirlo.** El probe marca backends como `detected` o `expected`; solo un benchmark que calcule tensores los pasa a `verified`.
- **Nunca afirmar una mejora sin baseline** en un solo nodo.
- **No reimplementar lo que existe.** Red: iroh. Tensores y autodiff: Burn. LLM repartido: llama.cpp RPC. Si falta algo, contribuir upstream.
- **El agente no ejecuta comandos arbitrarios recibidos por red.** Solo tipos de job de una lista permitida.
- **`rpc-server` de llama.cpp nunca se expone fuera de la LAN.**
- **Pesos y modelos no entran al repo.** Respetar sus licencias (Hunyuan3D tiene restricciones territoriales y de uso).
- Las decisiones que cambien la arquitectura se registran como ADR nuevo; no se reescriben los anteriores.

## Convenciones

- Documentación y mensajes al usuario en español; identificadores de código en inglés.
- Lógica pura separada de la E/S y con tests. Los parsers de salida de comandos son funciones puras probadas con muestras.
- El código de las tres plataformas se compila en todas (se elige en tiempo de ejecución), para que un cambio hecho en Windows no rompa macOS o Linux sin que se note.
- Sin `unwrap()` fuera de tests. Errores con contexto mediante `anyhow` en los binarios.

## Comprobaciones antes de dar algo por terminado

```bash
cargo fmt --all
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

## Hardware conocido

Ver [docs/hardware-matrix.md](docs/hardware-matrix.md). El nodo AMD (RX 6600) solo tiene Vulkan: no planificar PyTorch-GPU ni ROCm en él.
