# Universal AI Compute Cluster — Investigación y propuesta arquitectónica

**Fecha:** 2026-11-11  
**Estado:** Diseño de investigación (no ejecutado ni medido en los dispositivos del usuario).  
**Alcance:** Nodos macOS Apple Silicon, NVIDIA RTX 3070 8 GB y Radeon AMD 8 GB (modelo exacto pendiente); AWS opcional. Inferencia, entrenamiento y ajuste fino; LLM, imagen, video y 3D.

## 0. Resumen ejecutivo

**Conclusión:** no hay en 2026 un runtime único, universal y probado que convierta las memorias heterogéneas de Apple Metal + NVIDIA CUDA + Radeon AMD en un dispositivo de memoria unificada transparente para *cualquier* modelo y también permita entrenamiento arbitrario. Sí existen motores y compiladores que resuelven partes fundamentales. La plataforma debe combinar **control-plane nativo**, **data-plane de tensores**, **planificador consciente de red**, **adaptadores de modelos** y **motores existentes**; después introducir capacidades propias de distribución fina.

### Recomendación tecnológica

- **Núcleo nativo del producto:** Rust (Tokio + tonic/gRPC + axum; QUIC como transporte opcional). Servicio local en cada nodo, compatible con Windows/macOS/Linux, sin instalar una pila de Kubernetes.
- **Entrenamiento y ecosistema de modelos:** PyTorch y Python como motor de referencia, con adaptadores Hugging Face, Diffusers y código de modelos 3D. Mantener cada proceso de entrenamiento en backends soportados y probar heterogeneidad de forma incremental.
- **Motor Rust experimental:** Burn (CubeCL, autodiff, entrenamiento, CUDA/ROCm/Metal/Vulkan y backend remoto beta) como la mejor base a estudiar para operaciones tensoriales nativas y diferenciables.
- **Compiladores de operaciones:** IREE/MLIR como principal candidato cross-vendor; TVM Relax + Disco como referencia de planificación y ejecución distribuida. Evitar reescribir BLAS, kernels y autodiferenciación por principio.
- **Plugins de inferencia especializados:** ggml/llama.cpp RPC (LLMs y VLMs soportados); vLLM (servidores Linux y hardware compatible); ONNX Runtime (exportación/portabilidad); MLX en Mac; generadores 3D con Python/PyTorch.
- **Trabajos y nube:** El planificador Rust es la fuente de verdad. Ray Train/Serve o dstack se integran **opcionalmente como backends de ejecución** para trabajos Linux/nube, nunca como segundo planificador concurrente que reclame los mismos recursos.
- **Interfaz de usuario:** SvelteKit para dashboard, Tauri 2 si se desea aplicación nativa. Python SDK, CLI y API HTTP. Observabilidad con OpenTelemetry + Prometheus/Grafana opcionales.

## 1. Cuatro significados diferentes de «sumar memoria»

1. **Federación de recursos:** una sola API muestra y asigna múltiples GPU/CPU/RAM. Alcanzable para los tres dispositivos.
2. **Paralelismo entre tareas / datos:** cada nodo procesa una parte de los trabajos o lotes de datos. Alcanzable con buen soporte y sin «fusionar» memorias físicas.
3. **Sharding de un único modelo:** dividir pesos, activaciones y/o estados del optimizador. Viable para arquitecturas e implementaciones adaptadas; depende de velocidad, red y operaciones soportadas.
4. **Un único espacio virtual coherente de VRAM para cualquier operación:** no resuelto de forma general en tres máquinas heterogéneas; los accesos remotos necesitan comunicación y gestión explícitas o mediadas por el runtime. No prometer.

**Memoria física distinta de memoria utilizable:** una RTX 3070 de 8 GB + una Radeon de 8 GB proporcionan 16 GB nominales brutos entre ambas, no una GPU de 16 GB. La Mac M4 tiene memoria unificada con capacidad **aún sin verificar**; además deben reservarse buffers, sistema, cachés, activaciones y staging. Los modelos completos de entrenamiento usan memoria para parámetros, gradientes, activaciones y optimizer states; pueden consumir varias veces el tamaño de los pesos en inferencia.

## 2. Panorama: proyectos principales y conclusiones

| Proyecto | Qué funciona / diseño | Qué NO resuelve | Rol propuesto |
|---|---|---|---|
| [Apache Spark/MLlib](https://spark.apache.org/docs/latest/ml-guide) / [TorchDistributor](https://spark.apache.org/docs/latest/api/python/reference/api/pyspark.ml.torch.distributor.TorchDistributor.html) | DAG de datos, trabajos, pipeline ML; lanzamiento de entrenamiento PyTorch | VRAM unificada ni sharding de kernels por sí mismo | Modelo conceptual y posible integración de datos |
| [Ray](https://github.com/ray-project/ray) / [Ray Train](https://docs.ray.io/en/latest/train/train.html) | Actors, tasks, object store, grupos de recursos, training/serving | Sin memoria GPU automática; clúster multiplataforma Mac/Windows experimental | Adaptador de trabajos, especialmente Linux/AWS |
| [dstack](https://github.com/dstackai/dstack) | Aprovisionamiento de GPU en nube/on-prem, servicios y trabajos | No particiona grafos ni tensores de una inferencia por sí solo | Referencia/alternativa de gestión de infraestructura |
| [NVIDIA Dynamo](https://github.com/ai-dynamo/dynamo) | Separación coordinador/engine, disaggregation, enrutamiento, caché y escalado; Rust | Sesgo hacia centros de datos NVIDIA y motores de inferencia; no entrenamiento universal | **Referencia de diseño de control-plane** |
| [EXO actual](https://github.com/exo-explore/exo) | Descubrimiento, planificación topológica, MLX, paralelismo entre Macs y enlaces rápidos | README actual: GPU Linux todavía en desarrollo; no todos los backends | Inspiración de experiencia de usuario y planificación |
| [llama.cpp RPC](https://github.com/ggml-org/llama.cpp/blob/master/tools/rpc/README.md) | Servidores de dispositivos GGML remotos, pesos/KV repartidos, caches | PoC frágil, **inseguro en red abierta**, esencialmente familias GGML | Plugin, código para aprender placement |
| [Nakshatra](https://github.com/fthrvi/nakshatra) | Cadena de capas en máquinas heterogéneas, gRPC, llama.cpp parcheado | Solo arquitecturas soportadas por parche; no runtime universal | Referencia de layer pipeline, seguridad y leases |
| [Petals](https://github.com/bigscience-workshop/petals) | Divide bloques de Transformer por pares; inferencia y ajuste fino parcial | Más latencia, riesgos de privacidad en pares públicos, arquitectura limitada | Referencia de ejecución y entrenamiento remotos |
| [vLLM](https://github.com/vllm-project/vllm) | TP/PP/DP/EP, serving LLM eficiente | No entrenamiento genérico, heterogeneidad Mac/CUDA/AMD no automática | Plugin de LLM en nodos compatibles |
| [MLX](https://github.com/ml-explore/mlx) | Memoria unificada local Apple, autodiff, Metal, ring TCP y JACCL RDMA | Apple-centric; memoria unificada es local, no entre todos los fabricantes | Plugin Mac y referencia de comunicaciones |
| [PyTorch Distributed/FSDP2](https://docs.pytorch.org/docs/stable/distributed.fsdp.fully_shard.html) | Sharding de parámetros/gradientes/optimizer; DTensor, autograd | MPS no tiene ruta estándar distribuida con NCCL/Gloo; misma API no garantiza mezcla GPU | **Motor principal de entrenamiento compatible** |
| [DeepSpeed](https://github.com/deepspeedai/DeepSpeed/blob/master/docs/_tutorials/zero.md) | ZeRO-3, offload CPU y NVMe para entrenamiento | Centrado en entornos distribuidos compatibles y alta comunicación | Plugin Linux training |
| [Megatron-LM](https://github.com/NVIDIA/Megatron-LM) | Tensor/pipeline/context/expert parallel, checkpoints | Complejidad y sesgo de hardware NVIDIA | Patrones y algoritmos, no base portable |
| [Burn](https://github.com/tracel-ai/burn) | Rust, autodiff, entrenamiento, backends CUDA/ROCm/Metal/Vulkan/WebGPU, remote beta | No todos los modelos PyTorch se ejecutan sin port; remote beta no crea cluster tensorial transparente | **Candidato Rust nativo para subruntime** |
| [IREE](https://iree.dev/) | MLIR, importadores y targets CPU/CUDA/ROCm/Vulkan/Metal | No hace automáticamente entrenamiento end-to-end/multi-host con autograd | **Compilar subgrafos portables** |
| [TVM Relax/Disco](https://tvm.apache.org/docs/arch/) | Partición de tensores, DRef, workers, collectives y SocketSession | Rutas optimizadas NCCL/RCCL, mezcla heterogénea no garantizada | **Referencia fuerte para runtime distribuido** |
| [ONNX Runtime](https://onnxruntime.ai/docs/execution-providers/) | Muchos execution providers y CPU fallback | Prioriza inferencia, no un sistema de entrenamiento distribuido universal | Compatibilidad con modelos exportables |
| [Modular MAX/Mojo](https://max.modular.com/) | Unifica API de inferencia entre vendors y kernels optimizados | Compatibilidad y cobertura variables por SO/hardware y modelo; Windows no oficial | Pista de investigación, benchmark, no dependencia núcleo |
| [OpenUCX](https://github.com/openucx/ucx) | Transferencias TCP, RDMA, CUDA y ROCm en Linux/HPC | En macOS y Radeon sin ROCm no sustituye protocolo universal | Plugin de transporte optimizado Linux |

### Cómo lo hacen y qué aprender de cada patrón

**EXO / MLX:** usa descubrimiento y medición de topología, divide modelos en particiones y selecciona estrategia de paralelismo. Lo valioso es optimizar según enlaces y recursos, no asignar por VRAM solamente. Lo que no debemos copiar: asumir una sola familia de GPU o que RDMA/Thunderbolt sirva en Windows y macOS indistintamente.

**GGML RPC / llama.cpp:** servidores remotos exponen dispositivos GGML. El anfitrión planifica partes del modelo, pesos y KV, con caché remota para no reenviar pesos. Muy útil para prototipos de LLM; tiene advertencias explícitas de seguridad/fragilidad. No usar RPC sin encapsular en red privada segura.

**Nakshatra y Petals:** un trabajador procesa un intervalo de capas y envía estados ocultos al siguiente. Esto reduce tráfico por token de decodificación, a costa de saltos por red, pérdidas de disponibilidad y dificultades de admisión, recuperación y consistencia. Petals aporta ideas de DHT y fine-tuning remoto; Nakshatra, enrutamiento y arrendamiento de GPUs privadas.

**vLLM / Megatron:** el paralelismo de tensores necesita comunicación frecuente de operaciones colectivas; para redes lentas es habitual preferir pipeline paralelo entre nodos, y TP dentro de equipos con interconexión rápida. vLLM recomienda PP si no hay NVLink en el mismo nodo. El training incluye comunicación en backward y checkpointing.

**DeepSpeed ZeRO y PyTorch FSDP2:** no crean VRAM única; particionan estados y reúnen temporalmente parámetros, después los redistribuyen. Se debe controlar costo de all-gather, reduce-scatter y backward. DeepSpeed agrega offload a RAM/NVMe: más capacidad a cambio de menor velocidad.

**TVM Disco:** la interfaz de referencias distribuidas (DRef), programas compilados y collectives es muy similar a la abstracción que necesitamos; revisar su código antes de duplicar un sistema de runtime distribuido. La documentación indica NCCL/RCCL para collectives, así que comprobar antes la mezcla Apple+CUDA+AMD.

**Burn:** alternativa importante a un diseño Rust/C++ completamente propio: su API de tensor permite autodiff y backend remoto. Debe probarse portando un pequeño modelo con autograd a dos nodos, no suponer que un único objeto `Device::remote` hace partitioning entre varias GPUs.

**IREE:** buen backend para compilar subgrafos IR en varias arquitecturas; no sustituye a un planificador de clúster ni a un sistema de distribución de entrenamiento.

**NVIDIA Dynamo:** importante hallazgo arquitectónico: un framework de cómputo distribuido puede ser un plano de control sobre motores especializados, sin reimplementar cada motor. Nuestro objetivo es implementar una versión independiente del fabricante que incluya entrenamiento.

## 3. Entrenamiento frente a inferencia — estrategias exactas

| Estrategia | Inferencia | Entrenamiento | Característica |
|---|---|---|---|
| Task parallelism | Sí | Sí (experimentos separados) | Ejecutar múltiples trabajos en nodos diferentes |
| Data parallelism / DDP | Sí (replicas) | Sí | Replicar parámetros, distribuir batches; gradientes mediante all-reduce |
| Fully sharded data parallel / ZeRO | Puntualmente | Sí | Fraccionar parámetros, gradientes y estados del optimizador; operaciones colectivas frecuentes |
| Pipeline parallelism | Sí | Sí, con forward y backward repartidos | Cada máquina almacena capas completas; menos mensajes, pero uso secuencial/burbujas y activaciones |
| Tensor parallelism | Sí | Sí | Divide multiplicaciones/operadores; demanda enlaces rápidos y kernels/collectives compatibles |
| Expert parallelism (MoE) | Sí | Sí | Distribuye expertos; tráfico all-to-all variable |
| CPU/NVMe offload | Sí | Sí | Capacidad con penalización de E/S y memoria |
| Federated training | Sí, según método | Sí, con restricciones | Modelos/gradientes agregados con baja frecuencia; no equivale al entrenamiento exacto de una sola GPU |

**Importante:** el camino multi-GPU clásico de `torch.distributed` no funciona automáticamente entre CUDA y MPS. La documentación de Hugging Face indica que Gloo/NCCL distribuido no funciona en `mps` para ese flujo. Para los tres equipos debe desarrollarse un puente experimental de etapas/autograd, o iniciar por trabajos independientes y entrenamiento sobre una GPU/nodos compatibles.

## 4. Plataforma recomendada — separación de planos

```text
[Web SvelteKit / App Tauri / Python SDK / CLI / REST]
                            |
                      API Gateway
                            |
         Rust control plane (Tokio, tonic, axum)
         - registro de nodos / heartbeat / identidades
         - políticas de usuario, cuotas, prioridades
         - Job Scheduler (DAG, gang reservation)
         - Placement Planner (topología, VRAM, operaciones)
         - Model Registry y plan de partición
         - fallos, checkpoints y reintentos
                            |
                 Control RPC (mTLS gRPC)
               Data plane (QUIC/TCP stream)
                            |
          +-----------------+------------------+
          |                 |                  |
        Mac M4         NVIDIA RTX3070      AMD Radeon
      Rust agent         Rust agent         Rust agent
       Metal/MLX         CUDA/PyTorch     Vulkan, ROCm*
       Burn/IREE         GGML/vLLM*       Burn/IREE*
          |                 |                  |
        memoria         VRAM 8GB          VRAM 8GB
          +-----------------+------------------+
                            |
        Content-addressed store + checkpoints
                   local / S3 opcional
                            |
                AWS GPU worker opcional
```

`*` Según compatibilidad validada del sistema operativo, chip concreto, drivers y modelo. En el equipo AMD **no** presuponer ROCm hasta identificar la GPU. En Mac **no** confiar en que los contenedores Linux den aceleración Metal; iniciar con agente nativo.

### Control plane: Rust

- `tokio`: tareas asíncronas.
- `tonic` + Protobuf: estado de nodos, trabajos, lease y control.
- `axum`: API HTTP compatible con servicios y panel.
- `rustls` + mTLS: autenticación de nodos; claves locales.
- `quinn` (QUIC) / TCP: canal de datos en v1; negociación por capacidades.
- `sqlx` + SQLite en MVP (PostgreSQL después): registro de jobs, snapshots y auditoría.
- `opentelemetry`: trazas de trabajos y transferencias.

### Data plane: diseño para movimiento de tensores

Primero sobre TCP/QUIC con buffers tipados: `dtype`, `shape`, `strides`, `layout`, `device_id`, `tensor_id`, `partition_spec`, `compression`, `checksum`, `version`. Separar **datos voluminosos** del control: evitar circular grandes tensores por JSON. Permitir pipeline de transferencia y ejecución asincrónica. Optimizar gradualmente con UCX en nodos Linux/CUDA/ROCm soportados, pero conservar ruta TCP universal.

- `all_reduce`, `all_gather`, `reduce_scatter`, `broadcast`, `send/recv` y transferencias puntuales como primitivas del runtime.
- Evitar inventar collectives desde cero para producción: encapsular NCCL, RCCL, MLX ring y Gloo donde estén disponibles.
- El driver de Metal/CUDA/Vulkan debe ser **un adaptador de ejecución**. Para PyTorch mantener sidecar Python local; para Burn e IREE usar ejecución nativa donde cubran operadores.

### Planner de dos niveles

1. **Job scheduling:** quién ejecuta cada trabajo, reservas, deadlines, colas y prioridades; puede funcionar desde el MVP.
2. **Tensor/graph placement:** subgrafos o capas, dependencia y ubicación de pesos/activaciones. Costo aproximado: `tiempo_compute + bytes_transferidos / ancho_banda_efectivo + latencia_por_mensaje + costo_conversiones + costo_carga`.

El planificador debe tener presupuesto de memoria **reservado**, no ocupar 100% de VRAM. Registrar capacidad libre, concurrencia y latencia actual; preferir nodos con recursos libres y menor tráfico. El plan de partición incluye estado de entrenamiento (forward, backward, gradientes, optimizador) o inferencia (pesos, activaciones, KV/latentes).

### Formatos/model import

- Hugging Face Transformers/Diffusers y código Python como integración prioritaria, incluso cuando no pueden compilarse por completo.
- GGUF para LLM con ggml; safetensors para modelos que lo soporten; ONNX / StableHLO / MLIR para grafos exportables. Ninguno de estos formatos representa universalmente todo código Python personalizado.
- Subgrafos que no compilan deben usar fallback explícito al backend original o rechazar el plan con diagnóstico, **nunca afirmar compatibilidad universal**.

## 5. Red y límites físicos

Máximos teóricos antes de overhead:

| Red | Bytes/s aprox. | Transferir 4 GB, solo datos |
|---|---:|---:|
| Ethernet 1 Gb/s | 0.125 GB/s | >=32 s |
| Ethernet 2.5 Gb/s | 0.3125 GB/s | >=12.8 s |
| Ethernet 10 Gb/s | 1.25 GB/s | >=3.2 s |

En la práctica serán tiempos peores. GPUs locales tienen un ancho de banda interno de memoria mucho mayor. La inferencia LLM por capas mueve relativamente pocos datos por token comparado con pesos completos; entrenamiento y difusión pueden mover activaciones/gradientes grandes muchas veces por paso. En redes domésticas, **pipeline paralelo** y procesamiento por tareas suelen tener mejor relación costo-beneficio que tensor parallelism fino.

**AWS:** no ubicar una etapa que se comunica decenas de veces por segundo con tu RTX local sobre Internet. Mejor enviar el trabajo completo o el subpipeline a AWS, trabajar allí y descargar resultados/checkpoints, o crear grupos de nodos en una sola zona/red rápida.

## 6. Nuestros tres nodos: estrategias realistas

| Nodo | Ruta inicial | Riesgos / verificación |
|---|---|---|
| Apple M4 | agente Rust nativo, MLX/Metal/Burn/IREE | identificar GB de memoria unificada, versión macOS y opciones de backends |
| RTX 3070 8 GB | CUDA y PyTorch como referencia | driver NVIDIA, Windows vs WSL/Linux, memoria real libre, temperatura/energía |
| AMD Radeon 8 GB | Vulkan/CPU, posteriormente Burn o IREE; ROCm solo si compatible | **modelo exacto aún no confirmado**, drivers/arquitectura y soporte ROCm |

Para verificar antes de diseñar la partición: en Mac `sysctl -n hw.memsize` y `system_profiler SPHardwareDataType SPDisplaysDataType`; en NVIDIA `nvidia-smi`; en Windows PowerShell `Get-CimInstance Win32_VideoController | Select Name,AdapterRAM`, confirmar en Administrador de dispositivos; medir red con `iperf3` (hardware y permisos permiten instalación). Estos comandos **no equivalen a una prueba de GPU calculando tensores**.

## 7. Modelos 3D reales que motivan el proyecto

- [Hunyuan3D 2.1](https://github.com/Tencent-Hunyuan/Hunyuan3D-2.1): forma + texturas PBR; su README documenta 10 GB para forma, 21 GB para texturas y 29 GB para el pipeline conjunto en condiciones estándar (cifras del proyecto). Los pesos pueden descargarse, pero la **Tencent Hunyuan Community License** restringe territorios y ciertos usos; *source-available* no equivale a licencia open source permisiva.
- [TRELLIS](https://github.com/microsoft/TRELLIS): README indica NVIDIA con >=16 GB para referencia.
- [TRELLIS.2](https://github.com/microsoft/TRELLIS.2): modelo 4B; README exige NVIDIA con >=24 GB, probado Linux/A100/H100. Su código de entrenamiento distribuido no implica soporte inmediato para AMD+Apple.
- [Shap-E](https://github.com/openai/shap-e): ejemplo abierto MIT útil para primeras pruebas de un flujo 3D más ligero, no reemplazo visual AAA.

**Consecuencia práctica:** el primer experimento universal de tensor sharding debería usar un grafo controlado de PyTorch o Burn; el primer 3D realista debe ejecutarse como *trabajo completo en un nodo compatible* (por ejemplo AWS) hasta demostrar un plan experimental de reparto con resultados numéricos correctos.

## 8. Estructura inicial de repositorio

```text
universal-ai-cluster/
  Cargo.toml
  apps/
    desktop/                # Tauri 2 opcional
    web/                    # SvelteKit
  crates/
    uac-agent/              # hardware probe, heartbeats, leases, workers
    uac-controller/         # API, scheduler, registry
    uac-protocol/           # protobuf y tipos de datos
    uac-planner/            # cost model, memory budgets, placements
    uac-transport/          # QUIC/TCP, chunking, checksums
    uac-storage/            # blobs, datasets, checkpoints
    uac-telemetry/          # métricas
    uac-tensor/             # experimento: tipos tensores y operaciones
  plugins/
    torch-worker/           # PyTorch / Diffusers / modelos 3D
    burn-worker/            # Burn, autodiff
    ggml-worker/            # llama.cpp RPC / GGML
    mlx-worker/             # Mac
    iree-worker/            # MLIR/IREE
    tvm-disco-worker/       # investigación y benchmark
    ray-jobs/               # adaptador trabajos Linux/AWS
  sdk/
    python/
    rust/
  benches/
    hardware/
    network/
    ops/
    inference/
    training/
  tests/
    distributed/
    security/
    chaos/
    numerical/
  docs/
    adr/
    hardware-matrix.md
    benchmark-methodology.md
    model-compatibility.md
```

## 9. Contrato mínimo de API

```http
GET  /v1/cluster/nodes
GET  /v1/cluster/topology
GET  /v1/models
POST /v1/jobs
GET  /v1/jobs/{id}
POST /v1/jobs/{id}/cancel
GET  /v1/jobs/{id}/artifacts
POST /v1/benchmarks
GET  /v1/metrics
```

Ejemplo de trabajo:

```json
{
  "kind": "train",
  "framework": "pytorch",
  "model": "model-or-repository-id",
  "mode": "data_parallel_or_pipeline",
  "resources": {"gpu_required": true, "max_nodes": 2},
  "policy": {"prefer_local": true, "allow_cloud": false},
  "constraints": {"max_transfer_gb": 3.0}
}
```

`mode` es solicitud; si no hay backend compatible, el API debe devolver un **motivo de incompatibilidad**, no iniciar un entrenamiento presumiblemente distribuido.

## 10. Fases de implementación con condiciones de aceptación

**Fase 0 — Auditoría de hardware y benchmark base (1–2 semanas).** Inventario de SO/GPU/memoria/libres/drivers, 1/2.5/10 GbE o Wi-Fi, GPU matmul local por backend, latencia y ancho de banda con tamaños 1 KB/1 MB/100 MB, baseline en CPU/GPU. **Gate:** matriz reproducible; AMD identificado; no asignar capacidades por especulación.

**Fase 1 — Clúster nativo de trabajos (2–4 semanas).** Rust agents, TLS, registro, leases, prioridad, procesos de usuario, fallback y resultados reproducibles. PyTorch/Blender en nodos locales, AWS solo como worker separado. **Gate:** tarea para tres nodos, cancelación, reconexión, sin pérdida de artefactos; el coordinador no ejecuta código arbitrario en nodos no autorizados.

**Fase 2 — Primera inferencia partida real (2–4 semanas).** GGML/llama.cpp RPC en dos nodos compatibles, medir latencia y uso VRAM por máquina. Alternativa: pipeline de capas propio con pequeño Transformer y transferencias explícitas. **Gate:** modelo cuyo peso no cabe en una de las GPU pero cabe repartido; igualdad aproximada con baseline; `tokens/s`, VRAM por nodo y bytes por token medidos.

**Fase 3 — Entrenamiento heterogéneo experimental (4–8 semanas).** Burn remoto/autodiff y PyTorch sidecar; empezar con MLP/CNN de juguete y luego Transformer pequeño. Probar forward/backward por etapas, optimizador y checkpoint reproducible. **Gate:** gradientes dentro de tolerancia frente a referencia local; pérdida decreciente; recuperación de fallo. **No prometer FSDP automático CUDA+Metal.**

**Fase 4 — Runtime multimodal (4–8+ semanas).** Exportar subgrafos PyTorch/ONNX/StableHLO, compilar con IREE y estudiar Disco; ejecutar operadores de convolución, atención, VAE, difusión y generadores 3D. **Gate:** un modelo de difusión o 3D ejecuta al menos dos etapas reales en nodos distintos y entrega resultados comparables; velocidad medida vs solo GPU y vs nube.

**Fase 5 — Cloud/observabilidad/producción (2–5+ semanas).** S3/checkpoints, cola, reintentos, controles de costo, nodo AWS efímero, soporte de benchmark continuo, dashboards, políticas de seguridad. **Gate:** costos medidos, reproducibilidad, recuperación tras apagar un nodo, control de recursos y métricas.

Tiempos son **estimaciones de ingeniería** sujetas a resultados; no compromisos.

## 11. Pruebas obligatorias

### Correctitud
- Comparar tensores con referencia PyTorch CPU/CUDA y tolerancias definidas por precisión (`fp32`, `fp16`, `bf16`).
- Repetir resultados con seeds fijos; testear gradientes y optimizer step.
- Validar dimensiones, stride, layout, desbordamiento, tipos no soportados, endian entre hosts.

### Rendimiento
- Tiempo total, TTFT (LLM), tokens/s, imágenes/min o segundos/asset 3D, throughput entrenamiento (steps/s), utilización GPU, VRAM pico, RAM y red, tiempo en comunicación vs compute.
- Medir con un solo nodo, con dos y con tres; nunca afirmar mejora sin baseline.
- Control térmico, energía en portátiles, interferencia del SO, cargas activas.

### Resiliencia y seguridad
- Caída de worker durante forward/backward; tolerancia o cancelación consistente.
- Bloqueo de worker que no informa heartbeat, TTL/leases, cancelación por usuario.
- TLS mutuo, tokens efímeros, aislamiento de modelos, límites de descarga, controles de acceso.
- Verificación de checksums, hashes de pesos y artefactos; evitar cargar `pickle` no confiables sin aislamiento.
- No exponer ggml RPC en red pública; respeto a licencias de pesos y datasets.

## 12. Reglas para elegir el motor

- **LLM que cabe solo:** llama.cpp/MLX/vLLM según hardware, sin reparto por defecto.
- **LLM que solo cabe repartido:** GGML RPC o pipeline adaptado; favorecer pocas transiciones y cadenas por capas si la red es lenta.
- **Entrenamiento PyTorch normal:** un GPU compatible o Linux/NVIDIA homogéneo; si hay múltiples GPU compatibles, Ray Train + FSDP/DeepSpeed opcional.
- **Entrenamiento entre Mac+RTX+AMD:** modo investigación (forward/backward explícito o federación de trabajos), sin afirmar soporte plug-and-play.
- **Imagen/3D PyTorch:** ejecutar en el nodo que cumpla requisitos y descargar resultado; sharding cuando existan plugins de partición probados.
- **Cómputo nuevo escrito en Rust:** Burn, opcionalmente IREE para subgrafos compilados.
- **Nube:** AWS instancia efímera para un trabajo completo; evitar a menudo cruces de gradientes entre LAN y nube pública.

## 13. Riesgos que pueden invalidar la arquitectura

1. Radeon exacta no compatible con ROCm y kernels específicos: requiere Vulkan (menor cobertura/eficiencia) o CPU.
2. Multi-node macOS/Windows experimental en Ray; usar Rust nativo y Ray solo para workers Linux hasta validar.
3. Metal no ofrece NCCL y el entrenamiento distribuido estándar de PyTorch no se extrapola desde CUDA.
4. Ancho de banda doméstico insuficiente para paralelismo tensorial fino y difusión.
5. Pérdidas de compatibilidad con kernels custom de 3D; exportar ONNX/MLIR no arregla operadores faltantes.
6. GPU 8 GB no basta para TRELLIS.2 estándar ni Hunyuan3D 2.1 textura estándar; dividir es un proyecto de investigación.
7. Licencias y límites de uso de pesos Hunyuan: revisar su Tencent Community License separadamente de nuestras dependencias abiertas.
8. Entrenamiento completo de modelos grandes puede exigir más memoria para gradientes/optimizer/activaciones de lo que sugieren pesos cuantizados.
9. Duplicar runtime tensorial o planificador completo destruye tiempo de desarrollo; reutilizar kernels y solo escribir los componentes diferenciales.

## 14. Próximos pasos que debe ejecutar Codex

1. Crear monorepo `universal-ai-cluster`, un conjunto de ADR (decisiones), tests de CI Mac/Windows/Linux.
2. Escribir probes Rust para hardware/OS/NIC y una tabla de capacidades. Guardar resultados JSON versionados.
3. Preparar benchmarks `iperf3`, tensor matmul CPU/CUDA/Metal/Vulkan, replicables y documentados.
4. Implementar comunicación agent↔controller con `tonic` y mTLS; controlar heartbeats/leases.
5. Prototipar un pipeline de inferencia en dos nodos usando un grafo pequeño, verificar exactitud y costes de red.
6. Abrir spikes comparativos **Burn Remote vs IREE vs TVM Disco** para una operación y un backward, con decisión basada en pruebas.
7. Proponer checkpointing y API de entrenamientos; no entrenar en los tres equipos hasta validar compatibilidad.
8. Integrar 3D (Hunyuan/TRELLIS o alternativa) como jobs sobre un nodo que cumpla requisitos, antes de forzar multi-host.

## 15. Fuentes primarias recomendadas (documentación/repo)

**Arquitectura y coordinación**
- https://spark.apache.org/docs/latest/ml-guide
- https://spark.apache.org/docs/latest/api/python/reference/api/pyspark.ml.torch.distributor.TorchDistributor.html
- https://docs.ray.io/en/latest/train/train.html
- https://docs.ray.io/en/latest/ray-overview/installation.html
- https://github.com/ai-dynamo/dynamo
- https://github.com/dstackai/dstack

**Inferencia repartida**
- https://github.com/exo-explore/exo
- https://github.com/ggml-org/llama.cpp/blob/master/tools/rpc/README.md
- https://github.com/fthrvi/nakshatra/blob/main/docs/COMPONENTS.md
- https://github.com/bigscience-workshop/petals
- https://github.com/vllm-project/vllm/blob/main/docs/serving/parallelism_scaling.md
- https://github.com/ml-explore/mlx/blob/main/docs/src/usage/distributed.rst

**Entrenamiento**
- https://docs.pytorch.org/docs/stable/distributed.fsdp.fully_shard.html
- https://github.com/huggingface/accelerate/blob/main/docs/source/usage_guides/mps.md
- https://github.com/deepspeedai/DeepSpeed/blob/master/docs/_tutorials/zero.md
- https://github.com/NVIDIA/Megatron-LM/blob/main/docs/user-guide/parallelism-guide.md

**Compiladores, native e interconexión**
- https://github.com/tracel-ai/burn
- https://iree.dev/
- https://iree.dev/docs/guides/deployment-configurations/
- https://tvm.apache.org/docs/arch/
- https://onnxruntime.ai/docs/execution-providers/index.html
- https://github.com/openucx/ucx
- https://max.modular.com/system-requirements/

**Modelos 3D y licencias**
- https://github.com/Tencent-Hunyuan/Hunyuan3D-2.1
- https://github.com/Tencent-Hunyuan/Hunyuan3D-2.1/blob/main/LICENSE
- https://github.com/microsoft/TRELLIS
- https://github.com/microsoft/TRELLIS.2
- https://github.com/openai/shap-e

---

### Alcance de verificación
Este documento es una **revisión de repositorios, documentación primaria y rutas técnicas seleccionadas** disponible al 11 de noviembre de 2026. No constituye auditoría línea por línea de todos los commits, ni benchmarks ejecutados en los equipos descritos; todos los hitos de compatibilidad y rendimiento necesitan laboratorio. Un sistema verdaderamente universal debe tratar operaciones/modelos no soportados como limitaciones explícitas, no suponer que `CUDA + Metal + Vulkan` significa soporte automático para *cualquier* red neuronal.
