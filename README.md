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

El panel muestra **un mapa del clúster**: este equipo y los demás que encuentre en la red, cada uno unido a este por una línea que indica su estado (GPU disponible, lista para usar, en uso, desconectado) y la latencia. Los equipos aparecen solos, sin escribir direcciones, y Windows avisa con una notificación cuando uno se conecta o se desconecta.

Desde el panel se puede:

- **Ejecutar un modelo.** Eliges un GGUF local (carpetas configuradas o biblioteca de Ollama) y el panel dice si cabe en esta GPU, si habrá que repartirlo o si no cabe. Al iniciarlo aparece un chat; su API es compatible con OpenAI en `http://127.0.0.1:8080/v1`.
- **Compartir mi GPU.** Deja que otro equipo de la red la use. Se elige por qué conexión: cable o Wi-Fi.
- **Unir la memoria del clúster con un solo botón.** Si el modelo elegido no cabe en esta GPU pero sí uniendo la de otros equipos, aparece «Iniciar uniendo la memoria del clúster». El programa elige por sí solo los equipos mínimos necesarios (los de más memoria primero, porque cada salto de red cuesta velocidad), comprueba que responden y arranca el modelo repartido. Para que un equipo entre, tiene que estar compartiendo su GPU: o pulsando «Compartir mi GPU» en su panel, o con la casilla «Compartir mi GPU sola al abrir el programa», que lo deja listo para siempre (usa el cable si hay uno).
- **Elegir a mano.** «Usar su GPU» en el nodo de un equipo lo reserva para el siguiente modelo que no quepa. Un modelo que cabe en esta GPU nunca se reparte.
- **Compartir el chat con otros equipos.** Casilla «Permitir que otros equipos de mi red usen este chat»: el modelo se abre por la conexión que elijas y se protege con una clave aleatoria que ves en el panel. En el otro equipo, el nodo de este aparece con «Abrir su chat»; pide la clave una vez y la recuerda.
- **Añadir un equipo por su IP**, para uno que solo tenga el servidor RPC de llama.cpp.
- **Cambiar la configuración**, que se guarda en `%APPDATA%\uac\config.json`.

### Dos equipos unidos por un cable Ethernet

Se puede unir el PC y la laptop con un cable de red normal, sin router. Cada equipo se asigna solo una dirección `169.254.x.x` (tarda hasta un minuto) y el Wi-Fi sigue dando Internet. Después:

1. Abre el programa en los dos equipos. Deben verse en el mapa.
2. En el equipo que presta su GPU: «Compartir mi GPU», eligiendo la conexión marcada como cable.
3. En el otro: «Usar su GPU», y luego inicia un modelo que no quepa en una sola GPU.

Si no se ven, lo primero que hay que mirar es el firewall: Windows trata un cable directo como red pública y puede bloquear el programa. Permítelo cuando pregunte.

### Seguridad

El panel solo escucha en `127.0.0.1`. El chat también, salvo que marques compartirlo: entonces escucha en la IP de red elegida y exige la clave en cada petición (sin clave o con una incorrecta responde 401). La clave no viaja en el anuncio de la red: se la das tú a quien quieras. Quien la tenga puede usar el modelo y tu GPU mientras esté en marcha; «Generar clave nueva» la invalida. Compartir la GPU por RPC, en cambio, no tiene contraseña: mientras esté activo, cualquiera en esa red puede usarla. Por eso la casilla de compartir automáticamente pide confirmación y se recomienda limitar el puerto en el firewall a las IP de tus otros equipos (ver [docs/hardware-matrix.md](docs/hardware-matrix.md)).

**Disco.** Un equipo que presta su GPU guarda en disco una caché de los pesos que recibe (`%LOCALAPPDATA%\llama.cpp\rpc`), del tamaño del modelo. Si le queda poco espacio, pon `"cache": false` en la sección `rpc` de su `config.json`: es más lento al repetir, pero no llena el disco. Solo se aceptan direcciones de red local. Los errores quedan en `%APPDATA%\uac\uac-node.log`.

Para ver la salida en una consola mientras se desarrolla, usa el build de depuración: `cargo run -p uac-agent --bin uac-node`. Para probar dos nodos en un mismo equipo, arranca el segundo con la variable `UAC_CONFIG_DIR` apuntando a otra carpeta (con otro `panel_port` en su `config.json`) y con `UAC_ADVERTISE_LOOPBACK=1`.

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

Todavía no hay colas ni jobs: empiezan en la Fase 1.

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
