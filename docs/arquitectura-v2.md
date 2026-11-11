# Universal AI Compute Cluster — Arquitectura v2 (revisión)

**Fecha:** 2026-11-11
**Revisa a:** [investigacion_arquitectura_cluster_ia_universal_2026.md](../investigacion_arquitectura_cluster_ia_universal_2026.md) (v1)
**Estado:** propuesta. Lo marcado «verificado» se comprobó hoy en la fuente primaria o en esta máquina; lo demás necesita laboratorio.

## 0. Veredicto

El **diagnóstico** de v1 es correcto y se mantiene: no existe VRAM unificada transparente entre Metal + CUDA + Radeon, hay cuatro significados distintos de «sumar memoria», y la plataforma debe ser un plano de control sobre motores existentes con gates honestos.

El **plan de implementación** de v1 no es el mejor camino. Cinco problemas:

1. **Demasiadas apuestas en paralelo.** 20 proyectos evaluados, 7 plugins y 8 crates antes de tener un solo resultado medido. Casi todos (Ray, dstack, Spark, Dynamo, vLLM, DeepSpeed, Megatron, UCX, MAX) son de centro de datos Linux/NVIDIA y ninguno corre en los tres nodos reales.
2. **Decide sin conocer el hardware.** Ya se conoce un nodo y cambia el diseño: es una **Radeon RX 6600 sin soporte ROCm** conectada por **Wi-Fi a unos 300 Mb/s** (sección 1).
3. **Construye lo que ya existe.** mTLS + gRPC + QUIC propio + chunking + almacén de blobs están resueltos por iroh 1.0. El puente de forward/backward entre backends que v1 planea escribir en la Fase 3 ya está en Burn 0.22/0.23.
4. **Elige PyTorch como motor de entrenamiento de referencia** aunque no puede usar la GPU del nodo AMD ni hacer entrenamiento distribuido sobre MPS. El único runtime con autodiff que cubre CUDA + Metal + Vulkan con una sola API es Burn.
5. **Ordena mal las fases.** La evidencia más barata (repartir un LLM con llama.cpp RPC, cero código propio, un día) está en la Fase 2, después de 2–4 semanas de plano de control. Debe ir primero: decide si el reparto por red es viable en esta LAN.

## 1. Hechos nuevos

| Hallazgo | Estado | Impacto |
|---|---|---|
| Este nodo: **Radeon RX 6600, 8 GB** (RDNA2), Ryzen 5 3400G, 32 GB RAM, Windows 11, Vulkan presente | Verificado en la máquina | El «modelo AMD pendiente» de v1 queda resuelto |
| La RX 6600 y la serie RX 6000 de consumo **no figuran** en las matrices ROCm de AMD: ni en la 7.2.1 para Radeon (Windows y Linux) ni en la 10.1.0 unificada, que solo lista RX 7600+/9000 y, de RDNA2, las PRO W6800 y V620 | Verificado en [7.2.1 Windows](https://rocm.docs.amd.com/projects/radeon-ryzen/en/latest/docs/compatibility/compatibilityrad/windows/windows_compatibility.html), [7.2.1 Linux](https://rocm.docs.amd.com/projects/radeon-ryzen/en/latest/docs/compatibility/compatibilityrad/native_linux/native_linux_compatibility.html) y [10.1.0](https://rocm.docs.amd.com/en/latest/compatibility/compatibility-matrix.html). AMD añade que una GPU no listada «might be community-enabled» en builds nightly | Nodo AMD = **Vulkan**. Sin PyTorch-GPU oficial. Hay builds comunitarios con `HSA_OVERRIDE_GFX_VERSION` [sin probar en esta tarjeta](https://github.com/ssubedir/RCOm-windows-gfx1030) |
| Única interfaz de red física activa aquí: **Wi-Fi 6**, enlace negociado entre 292 y 325 Mb/s en dos lecturas | Verificado en la máquina (velocidad de enlace, no ancho de banda medido) | Máximo teórico ~40 MB/s: 4 GB tardan al menos 100 s. Peor que la fila más lenta de la tabla de v1. Cablear Ethernet es tarea de la Fase 0 |
| **llama.cpp RPC**: sigue siendo «proof-of-concept… fragile and insecure», pero ya trae caché local de tensores (`-c`), reparto proporcional a memoria, `--tensor-split`, RDMA opcional y trazas `GGML_RPC_DEBUG` con bytes por comando | Verificado en el [README](https://github.com/ggml-org/llama.cpp/blob/master/tools/rpc/README.md) | Inferencia repartida disponible hoy sin código propio. **Vulkan como servidor RPC ya está validado en loopback** en la RX 6600 con salida idéntica a la local ([resultados](benchmarks/llama-bench.md)); falta probarlo entre máquinas |
| **exo** usa solo MLX; en Linux corre en CPU; Windows no aparece; «A Mac and a Linux machine don't join each other» | Verificado en el [README](https://github.com/exo-explore/exo) | exo no sirve para este clúster. Queda solo como referencia de UX |
| **Burn 0.22** (oct. 2026): backend remoto con transporte **iroh** por defecto y WebSocket, remoto multi-dispositivo, `DistributedContext` + `all_reduce`, transferencias entre backends que preservan el grafo de autodiff. **0.23-pre**: `burn-shard` y entrenamiento en pipeline GPipe/1F1B | Verificado en [releases](https://github.com/tracel-ai/burn/releases) y README. No está documentado que `all_reduce` funcione entre nodos remotos de backends distintos | Burn pasa de «experimental» a **runtime heterogéneo principal**. Se elimina `uac-tensor` |
| **iroh 1.0** (jun. 2026): QUIC, conexión por clave pública, hole punching, relay de respaldo, multipath | Verificado en [iroh.computer](https://iroh.computer/blog/v1) y docs.rs | Sustituye tonic + rustls/mTLS + quinn + descubrimiento |
| **Ray**: los clústeres multinodo solo se soportan en Linux; Windows/macOS «at your own risk» | Verificado en [docs](https://docs.ray.io/en/latest/ray-overview/installation.html) | Ray sale del plan |
| **prima.cpp** (ICLR 2026): planificador consciente de heterogeneidad sobre llama.cpp; los autores reportan 5–17× menos latencia por token que llama.cpp/exo | [Paper](https://arxiv.org/abs/2504.08791); cifras de los autores, soporte Windows/Vulkan sin confirmar | Evaluar en Fase 2 antes de escribir un planner de capas propio |
| **GPUStack**: gestor de clúster ya hecho para Mac + Windows + Linux con llama.cpp empaquetado | Fuentes secundarias; revisar docs oficiales | Cubre buena parte de «Fase 1 + 2» solo para LLM. Evaluarlo evita duplicar; nuestro diferencial es entrenamiento, multimodal y Burn |
| **Cake** (Rust): reparte bloques de Transformer y componentes de Stable Diffusion (UNet / CLIP / VAE) entre workers | [Repo](https://github.com/evilsocket/cake); experimental | Referencia directa para el reparto por etapas de la Fase 4 |
| 3D en 8 GB: TripoSG pide ≥ 8 GB (MIT); Stable Fast 3D ~6–7 GB; Hunyuan3D 2.x solo-forma ~6 GB con modo low-VRAM; TRELLIS.2 tiene reportes contradictorios (8 GB vs 24 GB) | TripoSG en su [README](https://github.com/VAST-AI-Research/TripoSG); el resto son fuentes secundarias | El primer 3D puede correr **local en la RTX 3070** como job completo, sin AWS |
| **stable-diffusion.cpp** corre Flux en Radeon vía Vulkan | Reportes comunitarios | Da trabajo de imagen útil al nodo AMD sin PyTorch |

## 2. Qué se mantiene de v1

- Las cuatro definiciones de «sumar memoria» y la regla de no prometer la cuarta.
- Plano de control en Rust (Tokio + axum), SQLite en el MVP, agente nativo por nodo, sin Kubernetes.
- Planner de dos niveles con presupuesto de memoria reservado.
- Contrato REST de la sección 9 y la regla «devolver motivo de incompatibilidad».
- Gates por fase, pruebas de correctitud numérica contra referencia, nunca afirmar mejora sin baseline.
- AWS solo para trabajos completos, nunca etapas que crucen LAN ↔ Internet muchas veces por segundo.

## 3. Qué cambia

| Tema | v1 | v2 | Motivo |
|---|---|---|---|
| Transporte | tonic/gRPC + mTLS + quinn + chunking propio | **iroh 1.x** para control y datos; **iroh-blobs** para pesos y checkpoints | Identidad por clave sin CA, NAT traversal (el worker AWS no necesita VPN ni puertos abiertos), verificación BLAKE3, mismo transporte que Burn remoto, no requiere `protoc` |
| Runtime entre fabricantes | PyTorch de referencia; Burn «experimental»; `uac-tensor` propio | **Burn** como runtime principal para todo lo que cruce fabricantes | Único con autodiff en CUDA + Metal + Vulkan; el puente de etapas ya existe upstream |
| PyTorch | Motor principal de entrenamiento | **Motor de jobs completos por nodo** (RTX/CUDA, Mac/MPS, AWS) | No hay GPU en el nodo AMD ni ruta distribuida sobre MPS |
| Nodo AMD | «Vulkan/CPU, ROCm si compatible» | **Vulkan fijo**: llama.cpp, stable-diffusion.cpp, Burn. Además aloja controller y almacén (32 GB RAM) | RX 6600 fuera de la matriz ROCm |
| Compiladores | IREE principal, TVM Disco como referencia, spikes comparativos | Fuera del camino crítico | Ninguno aporta autograd multi-host; tres spikes en paralelo diluyen el esfuerzo |
| Orquestadores externos | Ray / dstack opcionales | Fuera. Un único scheduler propio | Ray multinodo es solo Linux |
| vLLM, DeepSpeed, Megatron, UCX, MAX, ONNX Runtime | Plugins o referencias | Bibliografía | Linux/NVIDIA homogéneo; no aplican a estos tres nodos |
| Primera inferencia partida | Fase 2, tras el plano de control | **Fase 0.5**, manual, antes de escribir el control plane | Evidencia en un día que decide la viabilidad |
| Reparto de imagen/3D | Subgrafos con IREE | **Reparto por etapas del pipeline** (encoder de texto / denoiser / VAE en nodos distintos) | Pocas transferencias por imagen, tolera red lenta; Cake lo demuestra |
| Estructura | 8 crates + 7 plugins | 5 crates; los plugins se crean cuando se usan | Menos superficie sin evidencia |

## 4. Arquitectura

```text
[CLI uac · Python SDK · REST + endpoint OpenAI-compatible · dashboard (después)]
                               |
              uac-controller  (Rust: axum + SQLite)
              registro · colas · leases · planner · tickets de unión
                               |
              iroh 1.x  (QUIC, identidad por clave, hole punching, relay)
              control: mensajes versionados     datos: iroh-blobs (BLAKE3)
                               |
     +----------------+--------+---------+---------------------+
     |                |                  |                     |
   Mac M4         RTX 3070 8 GB     RX 6600 8 GB (este)    AWS efímero
   uac-agent      uac-agent         uac-agent              uac-agent
   Metal          CUDA              Vulkan                 CUDA
   llama.cpp      llama.cpp         llama.cpp              jobs completos
   Burn           Burn              Burn                   PyTorch
   PyTorch MPS    PyTorch CUDA      stable-diffusion.cpp
   MLX (local)    Diffusers / 3D    controller + almacén
```

**Agente.** Un binario Rust por nodo. Sondea hardware, mantiene el endpoint iroh, supervisa procesos de motor (`rpc-server`, `llama-server`, servidor remoto de Burn, sidecar Python) y transmite logs y artefactos. Solo ejecuta **tipos de job de una lista permitida**; nunca comandos arbitrarios enviados por red.

**Controller.** Único scheduler y fuente de verdad. Puede vivir en el nodo AMD: tiene 32 GB de RAM y es el que menos cómputo GPU aporta.

**Unión al clúster.** El controller emite un ticket; el agente lo presenta una vez y su clave pública queda en la lista permitida. No hay CA ni certificados que rotar.

**Datos.** Pesos, datasets y checkpoints se direccionan por hash BLAKE3 con iroh-blobs. Los tensores en vuelo de un job repartido los mueve el motor (llama.cpp RPC o Burn remoto), no un data-plane propio.

**llama.cpp RPC.** Es inseguro por diseño: el agente lo enlaza a la interfaz LAN con una regla de firewall que solo admite a los pares del clúster. Tunelizarlo por iroh queda como mejora medida, no como requisito.

## 5. Reglas para elegir motor

| Carga | Motor | Dónde |
|---|---|---|
| LLM que cabe en un nodo | llama.cpp (o MLX en el Mac) | Nodo más pequeño donde quepa; sin reparto |
| LLM que solo cabe repartido | llama.cpp RPC | El menor número de nodos posible; preferir el Mac como anfitrión si tiene más memoria |
| Entrenamiento PyTorch normal | PyTorch, job completo | RTX, Mac o AWS |
| Entrenamiento entre fabricantes | Burn (remoto, pipeline GPipe/1F1B) | Modo investigación, con comparación de gradientes |
| Imagen | Diffusers (RTX/Mac), stable-diffusion.cpp (AMD) | Job completo; reparto por etapas en Fase 4 |
| 3D | PyTorch, job completo | RTX si cabe en 8 GB; si no, AWS |
| Varios trabajos a la vez | Scheduler | Un job por GPU: el beneficio más seguro del clúster |

## 6. Fases y gates

Detalle y tareas en [backlog.md](backlog.md).

| Fase | Objetivo | Gate |
|---|---|---|
| 0 Auditoría | Probe en los tres nodos, Ethernet, `iperf3` por pares | Matriz de hardware reproducible; ningún backend marcado sin evidencia |
| 0.5 Baseline | llama.cpp RPC manual en 2 y 3 nodos | tok/s, TTFT, VRAM por nodo y bytes por token medidos contra el mejor nodo solo |
| 1 Clúster de jobs | Agente + controller sobre iroh, colas, leases, artefactos | Job en tres nodos, cancelación, reconexión, sin perder artefactos |
| 2 Inferencia gestionada | El agente supervisa llama.cpp; endpoint OpenAI-compatible | Un modelo que no cabe en 8 GB se sirve repartido desde la API |
| 3 Entrenamiento heterogéneo | Spikes de Burn remoto y pipeline | Gradientes dentro de tolerancia frente a referencia local; pérdida decreciente; reanudación tras fallo |
| 4 Multimodal | Imagen y 3D como jobs; reparto por etapas | Un pipeline de difusión ejecuta dos etapas en nodos distintos con salida comparable |
| 5 Nube y operación | Worker AWS efímero, costes, dashboard | Costes medidos; recuperación al apagar un nodo |

## 7. Criterios de abandono

Decidirlos antes de medir evita racionalizar después.

- **Reparto de LLM.** Si en la Fase 0.5 el modelo repartido rinde menos de la mitad de tok/s que la mejor alternativa en un solo nodo (un cuantizado menor en una GPU, o el Mac solo), el reparto queda como opción manual y el proyecto se centra en federación de jobs.
- **Vulkan como servidor RPC.** Si el nodo AMD no funciona como `rpc-server` con anfitrión Metal o CUDA, participa solo con jobs completos.
- **Burn heterogéneo.** Si los gradientes de un modelo partido entre dos backends no coinciden con la referencia dentro de tolerancia fp32, se detiene la Fase 3 y se documenta el fallo upstream.
- **Wi-Fi.** Sin Ethernet en este nodo no se intenta ningún entrenamiento repartido que lo incluya.

## 8. Lo que este proyecto sí puede prometer

Con 8 GB + 8 GB + la memoria del Mac (aún sin medir), el Mac probablemente tiene más memoria utilizable que las dos GPU juntas. El valor realista, por orden de certeza:

1. Usar las tres GPU a la vez en trabajos distintos desde una sola cola.
2. Servir LLM de 9–20 GB que no caben en una GPU de 8 GB, con una penalización de velocidad que hay que medir.
3. Repartir por etapas pipelines de imagen/3D cuyas etapas caben por separado.
4. Entrenamiento partido entre fabricantes, como investigación.

## 9. No verificado

- Memoria y versión de macOS del Mac M4; sistema operativo y driver del nodo RTX.
- Que `ggml-rpc-server` con Vulkan funcione con anfitrión Metal/CUDA en otra máquina (en loopback sí funciona).
- Que `all_reduce` de Burn funcione entre nodos remotos de backends distintos.
- Rendimiento de iroh a más de 1 Gb/s (QUIC en espacio de usuario puede limitar por CPU).
- Compatibilidad de iroh-blobs con iroh 1.x en su versión actual.
- Cifras de VRAM de 3D distintas de TripoSG.
- Estado actual de torch-directml y de PyTorch sobre ZLUDA.

## 10. Fuentes

- AMD ROCm, matrices de compatibilidad Radeon: [Windows](https://rocm.docs.amd.com/projects/radeon-ryzen/en/latest/docs/compatibility/compatibilityrad/windows/windows_compatibility.html), [Linux](https://rocm.docs.amd.com/projects/radeon-ryzen/en/latest/docs/compatibility/compatibilityrad/native_linux/native_linux_compatibility.html)
- [llama.cpp RPC README](https://github.com/ggml-org/llama.cpp/blob/master/tools/rpc/README.md)
- [exo README](https://github.com/exo-explore/exo)
- [Burn releases](https://github.com/tracel-ai/burn/releases) y [README](https://github.com/tracel-ai/burn)
- [iroh 1.0](https://iroh.computer/blog/v1), [docs.rs/iroh](https://docs.rs/crate/iroh/latest)
- [Ray: instalación y soporte de plataformas](https://docs.ray.io/en/latest/ray-overview/installation.html)
- [prima.cpp (ICLR 2026)](https://arxiv.org/abs/2504.08791)
- [Cake](https://github.com/evilsocket/cake)
- [GPUStack](https://github.com/gpustack/gpustack)
- [TripoSG](https://github.com/VAST-AI-Research/TripoSG)
- [stable-diffusion.cpp](https://github.com/leejet/stable-diffusion.cpp)
- [ZLUDA, informe Q4 2025](https://phoronix.com/news/zluda-q4-2025-report)
- [ROCm comunitario para RDNA2 en Windows](https://github.com/ssubedir/RCOm-windows-gfx1030)
