# Backlog de desarrollo

Tareas ordenadas por fase. Cada una tiene un criterio de aceptación comprobable. Contexto y motivos en [arquitectura-v2.md](arquitectura-v2.md).

Nodos: **AMD** (RX 6600, esta máquina), **RTX** (RTX 3070), **MAC** (M4), **AWS**.

Estado: ☐ pendiente · ◐ en curso · ☑ hecho

## Fase 0 — Auditoría de hardware y red

| ID | Tarea | Nodo | Depende | Aceptación | Estado |
|---|---|---|---|---|---|
| T0.1 | Probe de hardware en Rust (`uac-agent probe`) | AMD | — | JSON con SO, CPU, RAM, GPU, VRAM real, NIC y herramientas | ☑ |
| T0.2 | Ejecutar el probe en MAC y RTX y guardar el JSON en `docs/hardware/` | MAC, RTX | T0.1 | Tres JSON versionados; corregir el probe si macOS/Linux fallan (rutas sin probar) | ☐ |
| T0.3 | Cablear Ethernet en AMD y registrar la velocidad de enlace de cada nodo | todos | — | Ningún nodo depende de Wi-Fi para datos | ☐ |
| T0.4 | `iperf3` entre cada par de nodos, en ambos sentidos | todos | T0.3 | Tabla de ancho de banda y latencia por par en `docs/hardware-matrix.md` | ☐ |
| T0.5 | Decidir SO del nodo RTX (Windows nativo, WSL2 o Linux) | RTX | T0.2 | ADR con la decisión | ☐ |
| T0.6 | `uac-agent bench`: matmul y transferencia host↔GPU por backend, con Burn | todos | T0.2 | Backends pasan de `detected`/`expected` a `verified` solo con cómputo medido | ☐ |

**Gate 0:** `docs/hardware-matrix.md` completo y reproducible; ningún backend marcado como utilizable sin evidencia.

## Fase 0.5 — Baseline de inferencia repartida (sin código propio)

| ID | Tarea | Nodo | Depende | Aceptación | Estado |
|---|---|---|---|---|---|
| T0.7 | Instalar llama.cpp en el **mismo build** (`b12110`, binarios oficiales, ver [llama-cpp.md](llama-cpp.md)): Vulkan (AMD), CUDA (RTX), Metal (MAC) | todos | — | `llama-bench` corre en cada nodo con el mismo GGUF; tok/s en [benchmarks/llama-bench.md](benchmarks/llama-bench.md) | ◐ AMD hecho; faltan RTX y MAC |
| T0.8 | Probar `ggml-rpc-server` Vulkan con anfitrión Metal y con anfitrión CUDA | AMD + 1 | T0.7, regla de firewall | Genera texto correcto, o queda documentado el fallo | ◐ validado en loopback (salida idéntica a la local); falta entre máquinas |
| T0.9 | Servir un GGUF que no cabe en 8 GB (hay uno de 15,66 GB en el nodo AMD): 2 nodos y 3 nodos | todos | T0.4, T0.8 | tok/s, TTFT, VRAM por nodo y bytes por token (`GGML_RPC_DEBUG=3`) | ☐ |
| T0.10 | Baseline de comparación: el mismo modelo con cuantizado menor en un solo nodo, y en el Mac solo | RTX, MAC | T0.7 | Tabla comparativa | ☐ |
| T0.11 | ADR de viabilidad aplicando el criterio de abandono | — | T0.9, T0.10 | Decisión escrita: reparto por defecto, opcional o descartado | ☐ |

**Gate 0.5:** números medidos de repartido frente a un solo nodo.

## Prototipo local (adelantado de las fases 1 y 2)

| ID | Tarea | Aceptación | Estado |
|---|---|---|---|
| P.1 | `uac-node`: icono de bandeja en Windows + panel web en 127.0.0.1 | Arranca sin consola, abre el panel, un segundo arranque solo abre el panel | ☑ (menú de bandeja sin probar a mano) |
| P.2 | Panel: hardware, GPU, red, llama.cpp, configuración persistente | Guarda en `%APPDATA%\uac\config.json` y valida la entrada | ☑ |
| P.3 | Arrancar/detener `llama-server` con un modelo local y abrir su chat | qwen2.5 3B responde por `/v1/chat/completions` en la RX 6600 | ☑ |
| P.4 | Arrancar/detener `ggml-rpc-server`; exponer en LAN solo con confirmación | Arranca en loopback; en IP de LAN exige `allow_lan` | ☑ |
| P.5 | Mismo prototipo en macOS (bandeja) y Linux | — | ☐ |
| P.7 | Descubrimiento automático de equipos por mDNS ([ADR 0006](adr/0006-descubrimiento-mdns.md)) | Dos instancias se ven en ambos sentidos; una caída sin aviso se detecta en ~10 s | ☑ en un solo equipo; ☐ entre dos equipos físicos |
| P.8 | Mapa del clúster en el panel, actividad y avisos | Estados y latencia visibles; aviso al conectarse o desconectarse un equipo | ☑ (notificaciones de Windows sin confirmar a la vista) |
| P.9 | «Usar su GPU»: repartir un modelo con un equipo descubierto | `llama-server` responde con `--rpc` hacia otra instancia | ◐ validado en loopback sobre la misma GPU; ☐ con dos GPU reales |
| P.10 | Usar el chat de un equipo desde otro (compartir en la LAN con clave) | Sin clave 401; con clave responde; el otro equipo ve el chat anunciado | ☑ por la IP de red de este equipo y en el navegador; ☐ entre dos equipos físicos |
| P.6 | Arranque automático con Windows (opcional) | — | ☐ |

## Fase 1 — Clúster de jobs

| ID | Tarea | Depende | Aceptación | Estado |
|---|---|---|---|---|
| T1.1 | Endpoint iroh en agente y controller; unión por ticket; lista permitida de claves | Gate 0 | Dos nodos se conectan por clave; una clave no listada es rechazada (test) | ☐ |
| T1.2 | Protocolo de control versionado en `uac-protocol`: Register, Heartbeat, Lease, JobAssign, JobStatus, Cancel | T1.1 | Tests de ida y vuelta de serialización; versión incompatible rechazada con mensaje claro | ☐ |
| T1.3 | Controller: registro de nodos, SQLite (`sqlx`), REST con axum (`/v1/cluster/nodes`, `/v1/jobs`…) | T1.2 | `GET /v1/cluster/nodes` lista los tres nodos con su probe | ☐ |
| T1.4 | Runner de jobs en el agente: tipos permitidos, supervisión de proceso, logs en streaming, cancelación | T1.2 | Job de prueba en cada nodo; cancelar mata el árbol de procesos en Windows y Unix | ☐ |
| T1.5 | Almacén de modelos y artefactos con iroh-blobs | T1.1 | Un GGUF se distribuye una vez, se verifica por hash y no se retransmite | ☐ |
| T1.6 | Scheduler: cola, prioridades, reserva de memoria con `uac-planner` | T1.3, T1.4 | Dos jobs que no caben juntos en una GPU se serializan; tres jobs van a tres nodos | ☐ |
| T1.7 | CLI `uac` (submit, status, cancel, logs, nodes) | T1.3 | Flujo completo desde terminal | ☐ |
| T1.8 | Pruebas de caos: matar agente, cortar red, expiración de lease | T1.6 | El job se reprograma o falla con estado consistente; sin artefactos perdidos | ☐ |
| T1.9 | CI en GitHub Actions: Windows, macOS, Linux (`fmt`, `clippy`, `test`) | — | Verde en los tres | ☐ |
| T1.10 | SDK Python mínimo sobre la API REST | T1.3 | `submit` + `wait` + descarga de artefacto | ☐ |

**Gate 1:** job en tres nodos, cancelación, reconexión, sin pérdida de artefactos; el agente rechaza tipos de job no permitidos.

## Fase 2 — Inferencia repartida gestionada

| ID | Tarea | Depende | Aceptación | Estado |
|---|---|---|---|---|
| T2.1 | `ggml-worker`: el agente lanza y supervisa `rpc-server` / `llama-server`; versión fijada; firewall restringido a pares | Gate 1, T0.11 | Arranque y parada limpios desde el controller | ☐ |
| T2.2 | El planner decide nodo único o reparto y calcula `--tensor-split` con presupuestos y enlaces medidos | T2.1 | Reproduce o mejora el reparto manual de T0.9 | ☐ |
| T2.3 | Endpoint OpenAI-compatible en el controller (proxy a `llama-server`) | T2.1 | Un cliente OpenAI estándar conversa con un modelo repartido | ☐ |
| T2.4 | Evaluar prima.cpp y GPUStack frente a lo construido | T2.2 | ADR: adoptar, integrar o descartar, con números | ☐ |
| T2.5 | Informe de rendimiento 1/2/3 nodos | T2.3 | Publicado en `docs/` | ☐ |

**Gate 2:** un modelo que no cabe en 8 GB se sirve repartido a través de la API.

## Fase 3 — Entrenamiento heterogéneo con Burn (investigación)

| ID | Tarea | Depende | Aceptación | Estado |
|---|---|---|---|---|
| T3.1 | Servidor remoto de Burn en cada nodo (Vulkan, CUDA, Metal); correr `remote-mnist` desde otra máquina | Gate 0 | Entrena en la GPU remota de cada nodo | ☐ |
| T3.2 | MLP partido en dos etapas sobre dos backends distintos: forward + backward | T3.1 | Gradientes dentro de tolerancia fp32 frente a referencia en un solo dispositivo | ☐ |
| T3.3 | `DistributedContext` / `all_reduce` entre nodos de backends distintos | T3.1 | Funciona, o queda issue upstream con reproducción mínima | ☐ |
| T3.4 | Transformer pequeño en pipeline (GPipe/1F1B, Burn 0.23) entre dos nodos | T3.2 | Pérdida decreciente; steps/s y tiempo en red medidos | ☐ |
| T3.5 | `burn-worker` como tipo de job; checkpoints al almacén; reanudación | T3.4, Gate 1 | Matar un nodo a mitad y reanudar desde checkpoint | ☐ |
| T3.6 | ADR: continuar, limitar o abandonar | T3.2–T3.4 | Decisión con datos | ☐ |

**Gate 3:** gradientes correctos, pérdida decreciente, recuperación de fallo.

## Fase 4 — Multimodal

| ID | Tarea | Depende | Aceptación | Estado |
|---|---|---|---|---|
| T4.1 | `torch-worker`: sidecar Python (uv) para jobs Diffusers en RTX y MAC | Gate 1 | Imagen generada como job, artefacto descargado | ☐ |
| T4.2 | stable-diffusion.cpp con Vulkan como tipo de job en AMD | Gate 1 | Imagen generada en la RX 6600 | ☐ |
| T4.3 | 3D como job completo en RTX (TripoSG; Hunyuan3D solo-forma si la licencia lo permite) | T4.1 | Malla generada; VRAM pico y segundos por asset medidos | ☐ |
| T4.4 | Reparto por etapas de un pipeline de difusión (encoder / denoiser / VAE) entre RTX y MAC | T4.1 | Salida comparable a un solo nodo con semilla fija; tiempo y bytes transferidos | ☐ |
| T4.5 | Revisar Cake como referencia o base | T4.4 | Nota técnica | ☐ |

**Gate 4:** dos etapas reales de un pipeline en nodos distintos con resultado comparable.

## Fase 5 — Nube y operación

| ID | Tarea | Depende | Aceptación | Estado |
|---|---|---|---|---|
| T5.1 | Worker AWS efímero: el agente en Linux se une por ticket, sin puertos entrantes | Gate 1 | Job completo en AWS y artefacto de vuelta en el almacén local | ☐ |
| T5.2 | Límites de coste y apagado automático | T5.1 | La instancia se destruye al terminar o al superar presupuesto | ☐ |
| T5.3 | Métricas (OpenTelemetry) y dashboard SvelteKit | Gate 2 | Nodos, colas, VRAM y red visibles | ☐ |
| T5.4 | App de escritorio Tauri (opcional) | T5.3 | — | ☐ |

## Por dónde empezar

1. T0.2 y T0.3: son físicas y desbloquean todo lo demás.
2. T0.7–T0.11 en paralelo con T1.1–T1.2: el baseline no necesita el control plane.
3. T3.1 puede empezar en cuanto exista Gate 0; es independiente de la Fase 1.
