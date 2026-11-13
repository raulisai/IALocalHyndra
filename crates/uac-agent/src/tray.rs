//! Icono en la bandeja del sistema de Windows (junto al reloj).
//!
//! Clic izquierdo o doble clic: abre el panel. Clic derecho: menú.

use std::time::{Duration, Instant};

use tao::event::{Event, StartCause};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};

enum UserEvent {
    Tray(TrayIconEvent),
    Menu(MenuEvent),
}

/// Cada cuánto se refresca el texto que aparece al pasar el ratón por el icono.
const TOOLTIP_REFRESH: Duration = Duration::from_secs(3);

/// Bloquea el hilo principal con el bucle de mensajes de Windows.
/// `tooltip` se consulta periódicamente para reflejar el estado del clúster.
/// `on_quit` se ejecuta una vez al elegir «Salir».
pub fn run(
    url: String,
    tooltip: impl Fn() -> String + 'static,
    on_quit: impl FnOnce() + 'static,
) -> ! {
    let event_loop = EventLoopBuilder::<UserEvent>::with_user_event().build();

    let proxy = event_loop.create_proxy();
    TrayIconEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Tray(e));
    }));
    let proxy = event_loop.create_proxy();
    MenuEvent::set_event_handler(Some(move |e| {
        let _ = proxy.send_event(UserEvent::Menu(e));
    }));

    let open_item = MenuItem::new("Abrir panel", true, None);
    let status_item = MenuItem::new(format!("Activo en {url}"), false, None);
    let quit_item = MenuItem::new("Salir (detiene los servicios)", true, None);
    let menu = Menu::new();
    let _ = menu.append_items(&[
        &open_item,
        &status_item,
        &PredefinedMenuItem::separator(),
        &quit_item,
    ]);

    let mut tray: Option<tray_icon::TrayIcon> = None;
    let mut on_quit = Some(on_quit);
    let mut shown_tooltip = tooltip();

    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::WaitUntil(Instant::now() + TOOLTIP_REFRESH);
        if let Some(icon) = &tray {
            let current = tooltip();
            if current != shown_tooltip {
                let _ = icon.set_tooltip(Some(&current));
                shown_tooltip = current;
            }
        }
        match event {
            Event::NewEvents(StartCause::Init) => {
                let built = TrayIconBuilder::new()
                    .with_menu(Box::new(menu.clone()))
                    .with_menu_on_left_click(false)
                    .with_tooltip(&shown_tooltip)
                    .with_icon(icon())
                    .build();
                match built {
                    Ok(icon) => tray = Some(icon),
                    Err(err) => crate::log_line(&format!("no se pudo crear el icono: {err}")),
                }
            }
            Event::UserEvent(UserEvent::Tray(TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            }))
            | Event::UserEvent(UserEvent::Tray(TrayIconEvent::DoubleClick { .. })) => {
                crate::open_browser(&url);
            }
            Event::UserEvent(UserEvent::Menu(e)) if e.id == open_item.id() => {
                crate::open_browser(&url);
            }
            Event::UserEvent(UserEvent::Menu(e)) if e.id == quit_item.id() => {
                if let Some(quit) = on_quit.take() {
                    quit();
                }
                tray.take();
                *control_flow = ControlFlow::Exit;
            }
            _ => {}
        }
    })
}

fn icon() -> Icon {
    let (rgba, size) = icon_rgba();
    // El búfer siempre tiene el tamaño correcto: `from_rgba` no puede fallar.
    Icon::from_rgba(rgba, size, size).expect("icono generado con tamaño válido")
}

/// Icono de 32×32 dibujado en código: círculo verde azulado con tres nodos
/// blancos unidos, para no depender de archivos de imagen.
pub fn icon_rgba() -> (Vec<u8>, u32) {
    const N: u32 = 32;
    let teal = [15u8, 143, 126];
    let nodes = [(16.0f32, 8.5f32), (8.5, 21.5), (23.5, 21.5)];
    let mut rgba = Vec::with_capacity((N * N * 4) as usize);
    for y in 0..N {
        for x in 0..N {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let in_circle = ((px - 16.0).powi(2) + (py - 16.0).powi(2)).sqrt() <= 15.5;
            let on_node = nodes
                .iter()
                .any(|&(nx, ny)| ((px - nx).powi(2) + (py - ny).powi(2)).sqrt() <= 3.6);
            let on_edge = (0..3).any(|i| {
                let (a, b) = (nodes[i], nodes[(i + 1) % 3]);
                segment_distance((px, py), a, b) <= 1.1
            });
            let pixel = if !in_circle {
                [0, 0, 0, 0]
            } else if on_node || on_edge {
                [255, 255, 255, 255]
            } else {
                [teal[0], teal[1], teal[2], 255]
            };
            rgba.extend_from_slice(&pixel);
        }
    }
    (rgba, N)
}

fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icon_buffer_has_right_size_and_shape() {
        let (rgba, n) = icon_rgba();
        assert_eq!(rgba.len(), (n * n * 4) as usize);
        let px = |x: u32, y: u32| &rgba[((y * n + x) * 4) as usize..][..4];
        assert_eq!(px(0, 0)[3], 0, "esquina transparente");
        assert_eq!(px(16, 8), [255, 255, 255, 255], "nodo superior blanco");
        assert_eq!(px(16, 27)[..3], [15, 143, 126], "fondo verde azulado");
    }
}
