# Universal AI Cluster

Plano de control para usar como un solo clúster tres equipos con GPU de fabricantes distintos: Mac M4 (Metal), RTX 3070 (CUDA) y Radeon RX 6600 (Vulkan), con AWS opcional.

No promete VRAM unificada. Promete, por orden de certeza: una cola única que usa las tres GPU a la vez, servir LLM que no caben en una sola GPU, repartir pipelines por etapas y, como investigación, entrenamiento entre fabricantes.

## Documentos

| Documento | Para qué |
|---|---|
| [docs/arquitectura-v2.md](docs/arquitectura-v2.md) | Arquitectura vigente y qué cambia respecto a la investigación inicial |
| [docs/backlog.md](docs/backlog.md) | Tareas por fase con criterios de aceptación |
| [docs/hardware-matrix.md](docs/hardware-matrix.md) | Inventario medido de los nodos |
| [docs/adr/](docs/adr/) | Decisiones de arquitectura |
| [investigacion_arquitectura_cluster_ia_universal_2026.md](investigacion_arquitectura_cluster_ia_universal_2026.md) | Investigación inicial (v1) |

## Probar el prototipo (Windows)

Requisitos: Rust y los binarios de llama.cpp en `tools/llama.cpp/b12110/` (ver [docs/llama-cpp.md](docs/llama-cpp.md)).

```bash
cargo build --release -p uac-agent --bin uac-node
```

Después ejecuta `target\release\uac-node.exe` (doble clic sirve). Aparece un icono verde en la bandeja, junto al reloj, y se abre el panel en `http://127.0.0.1:47800/`.

- **Clic izquierdo** en el icono: abre el panel.
- **Clic derecho**: menú con «Abrir panel» y «Salir». Salir detiene también los servidores de llama.cpp que haya arrancado.
- Si el programa ya está abierto, volver a ejecutarlo solo abre el panel.

Desde el panel se puede:

- ver el hardware del equipo, las GPU y sus backends, la red y la carpeta de llama.cpp;
- elegir un modelo GGUF local (carpetas configuradas y biblioteca de Ollama), arrancar `llama-server` y abrir su chat; su API es compatible con OpenAI en `http://127.0.0.1:8080/v1`;
- arrancar `ggml-rpc-server` para prestar la GPU a otro nodo;
- cambiar la configuración, que se guarda en `%APPDATA%\uac\config.json`.

El panel y `llama-server` solo escuchan en `127.0.0.1`. El servidor RPC también, salvo que se cambie su IP en la configuración y se confirme el aviso. Los errores quedan en `%APPDATA%\uac\uac-node.log`.

Para ver la salida en una consola mientras se desarrolla, usa el build de depuración: `cargo run -p uac-agent --bin uac-node`.

## Estado

Fase 0 en curso. Funciona hoy:

```bash
cargo run -p uac-agent -- probe --out docs/hardware/<nombre>.json
```

```bash
cargo run -p uac-controller -- nodes
```

```bash
cargo run -p uac-controller -- plan --required-gb 12
```

`probe` inventaría SO, CPU, RAM, GPU, VRAM real, red y herramientas. `nodes` muestra la tabla del clúster a partir de los informes guardados. `plan` dice si una carga cabe en un nodo, repartida o en ninguno.

Los informes de `probe` se quedan en local (están en `.gitignore`): contienen el nombre del equipo y rutas del usuario.

Todavía no hay red entre nodos gestionada por el programa, colas ni jobs: empiezan en la Fase 1. Para repartir un modelo entre equipos hoy, arranca el servidor RPC en un nodo y escribe su `ip:puerto` en «Nodos RPC para repartir» del otro.

## Estructura

```text
crates/
  uac-protocol/     tipos compartidos (informe de nodo; mensajes de control en T1.2)
  uac-probe/        inventario de hardware por plataforma
  uac-planner/      presupuestos de memoria, ubicación y coste de red
  uac-agent/        uac-node (bandeja + panel web) y uac-agent (CLI de probe)
  uac-controller/   binario del controller
docs/
```

## Desarrollo

```bash
cargo test --workspace
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo fmt --all
```

Requiere Rust 1.85 o posterior.
