# ADR 0002 — iroh como transporte de control y datos

**Estado:** propuesta · 2026-11-11

## Contexto

v1 propone tonic/gRPC + rustls/mTLS para control, quinn/TCP para datos, chunking y checksums propios y un almacén de blobs propio. Eso implica gestionar una CA, distribuir certificados, descubrir nodos y abrir puertos o montar VPN para el worker de AWS.

iroh llegó a 1.0 en junio de 2026: QUIC, conexión por clave pública, hole punching, relay de respaldo y multipath. Burn usa iroh como transporte por defecto de su backend remoto.

## Decisión

- iroh 1.x para todo el tráfico entre agente y controller.
- La identidad de un nodo es su clave pública. Unión por ticket de un solo uso; el controller mantiene la lista permitida.
- iroh-blobs para pesos, datasets y checkpoints (direccionados por BLAKE3).
- Mensajes de control serializados con un campo de versión de protocolo; ALPN propio.
- La API hacia usuarios sigue siendo HTTP con axum.
- El transporte queda detrás de un trait para poder añadir TCP directo en LAN si QUIC limita.

## Consecuencias

- Sin CA, sin `protoc`, sin VPN para AWS.
- Se pierde el ecosistema gRPC (reflexión, clientes generados en otros lenguajes). Los clientes externos usan la API REST.
- QUIC en espacio de usuario puede limitar por CPU por encima de ~1 Gb/s. Hoy el cuello es el Wi-Fi del nodo AMD (~300 Mb/s); medir cuando haya 2.5/10 GbE.
- Dependencia de los relays públicos de iroh para el caso AWS, salvo que se despliegue uno propio.

## Por verificar

- Versión de iroh-blobs compatible con iroh 1.x.
- Rendimiento real en LAN frente a TCP (`iperf3`).
