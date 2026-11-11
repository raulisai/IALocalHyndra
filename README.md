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

Todavía no hay red entre nodos, colas ni ejecución de jobs: empiezan en la Fase 1.

## Estructura

```text
crates/
  uac-protocol/     tipos compartidos (informe de nodo; mensajes de control en T1.2)
  uac-probe/        inventario de hardware por plataforma
  uac-planner/      presupuestos de memoria, ubicación y coste de red
  uac-agent/        binario por nodo
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
