# ADR 0006 — Descubrimiento en la red local con mDNS

**Estado:** aceptada para el prototipo · 2026-11-13

## Contexto

El prototipo necesita que un equipo vea a los demás sin escribir direcciones IP, también cuando dos equipos están unidos solo por un cable Ethernet, sin router ni DHCP. El ADR 0002 propone iroh para el tráfico entre agente y controller, pero ese plano de control todavía no existe y el prototipo no lo necesita para repartir un modelo: llama.cpp RPC ya mueve los tensores.

## Decisión

- Cada nodo anuncia el servicio `_uac._tcp.local.` con mDNS/DNS-SD (crate `mdns-sd`) y escucha los anuncios de los demás.
- Todo lo que un nodo sabe de otro viaja en el registro TXT del anuncio: nombre, sistema, GPU, memoria, backend, si comparte su GPU por RPC (`ip:puerto`) y qué modelo ejecuta. **No se abre ningún puerto TCP nuevo** para el descubrimiento.
- Se ignoran las interfaces virtuales (Hyper-V, WSL, Docker, VPN).
- mDNS tarda mucho en dar por perdido a un equipo que se apaga sin despedirse, así que cada 15 s se le pregunta de forma activa. En la prueba, un equipo matado sin aviso apareció como desconectado a los 10 s.
- La latencia que muestra el panel es el tiempo de abrir una conexión TCP al puerto RPC del otro equipo, cada 10 s y solo mientras este nodo no lo está usando.
- Entre varias direcciones de un mismo vecino se prefiere la que está en una red con cable antes que la de Wi-Fi.
- Las direcciones RPC, anunciadas o escritas a mano, solo se aceptan si son de red local (loopback, privadas o de enlace local).

## Consecuencias

- Funciona con cable directo: sin DHCP los equipos se asignan solos una dirección `169.254.x.x` y mDNS opera sobre ella.
- El anuncio es visible para cualquiera en la red local. Solo contiene datos no sensibles, pero revela que el equipo existe y qué GPU tiene. Se puede desactivar en la configuración.
- No hay identidad ni autenticación: un equipo de la red puede anunciarse con cualquier nombre. Es aceptable mientras el único efecto de «usar» un vecino sea conectarse a su RPC dentro de la LAN. La identidad por clave del ADR 0002 sigue siendo necesaria antes de aceptar jobs por red.
- No descubre equipos fuera de la LAN (el worker de AWS). Eso queda para iroh.
- El firewall de Windows puede bloquear mDNS en redes marcadas como públicas, que es como Windows clasifica un cable directo.

## Sin verificar

- Descubrimiento entre dos equipos físicos distintos: solo se probó con dos instancias en un mismo equipo.
- Comportamiento en macOS y Linux.
