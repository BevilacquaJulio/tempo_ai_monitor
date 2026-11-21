use crate::core::{
    monitor,
    preferences::{self, DesktopState, QuickBounds},
};
use std::{sync::atomic::Ordering, time::Duration};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};
use tauri_plugin_global_shortcut::GlobalShortcutExt;

const TRAY_ID: &str = "tempo-ai";

fn should_hide_quick_on_dashboard(pin_panel: bool) -> bool {
    !pin_panel
}

pub fn show_main(app: &AppHandle, settings: bool) {
    let pin_panel = app.state::<DesktopState>().config.lock().unwrap_or_else(|error| error.into_inner()).pin_panel;
    if should_hide_quick_on_dashboard(pin_panel) {
        if let Some(quick) = app.get_webview_window("quick") { let _ = quick.hide(); }
    }
    if let Some(main) = app.get_webview_window("main") {
        let _ = main.unminimize();
        let _ = main.show();
        let _ = main.set_focus();
        if settings { let _ = main.emit("open-settings", ()); }
    }
    monitor::queue(app, true);
}

fn place_panel(window: &WebviewWindow, anchor: Option<PhysicalPosition<f64>>) {
    let monitor = anchor.and_then(|point| window.monitor_from_point(point.x, point.y).ok().flatten())
        .or_else(|| window.primary_monitor().ok().flatten());
    let Some(monitor) = monitor else { return; };
    let Ok(size) = window.outer_size() else { return; };
    let area = monitor.work_area();
    let inset = (10.0 * monitor.scale_factor()) as i32;
    let min_x = area.position.x + inset;
    let min_y = area.position.y + inset;
    let max_x = (area.position.x + area.size.width as i32 - size.width as i32 - inset).max(min_x);
    let max_y = (area.position.y + area.size.height as i32 - size.height as i32 - inset).max(min_y);
    let x = anchor.map(|point| (point.x as i32 - size.width as i32 / 2).clamp(min_x, max_x)).unwrap_or(max_x);
    let _ = window.set_position(PhysicalPosition::new(x, max_y));
}

/// Where Quick Monitor goes: next to the tray by default, or where the user last dragged/resized it.
#[derive(Default)]
pub struct PanelPosition {
    placed: Option<PhysicalPosition<i32>>,
    saved: Option<QuickBounds>,
}

impl PanelPosition {
    pub fn seed(&mut self, bounds: QuickBounds) {
        self.saved = Some(bounds);
    }
}

fn open_position(window: &WebviewWindow, anchor: Option<PhysicalPosition<f64>>) {
    let state = window.state::<DesktopState>();
    let mut position = state.quick_position.lock().unwrap_or_else(|error| error.into_inner());
    // Restore saved position and size when still on a connected monitor.
    if let Some(bounds) = position.saved.filter(|b| {
        b.width >= 260
            && b.height >= 220
            && window.monitor_from_point(f64::from(b.x) + 16.0, f64::from(b.y) + 16.0).ok().flatten().is_some()
    }) {
        let _ = window.set_size(PhysicalSize::new(bounds.width, bounds.height));
        let point = PhysicalPosition::new(bounds.x, bounds.y);
        let _ = window.set_position(point);
        position.placed = Some(point);
        return;
    }
    position.saved = None;
    place_panel(window, anchor);
    position.placed = window.outer_position().ok();
}

pub fn toggle_quick(app: &AppHandle, anchor: Option<PhysicalPosition<f64>>) {
    if let Some(window) = app.get_webview_window("quick") {
        if window.is_visible().unwrap_or(false) { let _ = window.hide(); return; }
        open_position(&window, anchor);
        let _ = window.show();
        let _ = window.set_focus();
        let _ = window.emit("panel-opened", ());
        monitor::queue(app, true);
    }
}

pub fn initialize(app: &AppHandle) -> tauri::Result<()> {
    let portuguese = app.state::<DesktopState>().portuguese.lock().unwrap_or_else(|error| error.into_inner()).unwrap_or(false);
    WebviewWindowBuilder::new(app, "quick", WebviewUrl::App("index.html".into()))
        .title(if portuguese { "Monitor rápido do TempoAI" } else { "TempoAI Quick Monitor" })
        .inner_size(348., 420.).min_inner_size(300., 260.).resizable(true).maximizable(false)
        .skip_taskbar(true).always_on_top(true).decorations(false).visible(false).build()?;
    let menu = tray_menu(app, portuguese)?;
    let mut tray = TrayIconBuilder::with_id(TRAY_ID).tooltip("TempoAI").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() { tray = tray.icon(icon.clone()); }
    tray.on_menu_event(|app, event| match event.id.as_ref() {
        "dashboard" => show_main(app, false),
        "settings" => show_main(app, true),
        "quit" => { let _ = app.global_shortcut().unregister_all(); app.exit(0); },
        _ => {}
    }).on_tray_icon_event(|tray, event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, position, .. } = event {
            toggle_quick(tray.app_handle(), Some(position));
        }
    }).build(app)?;
    Ok(())
}

fn tray_menu(app: &AppHandle, portuguese: bool) -> tauri::Result<Menu<tauri::Wry>> {
    let open = MenuItem::with_id(app, "dashboard", if portuguese { "Abrir painel" } else { "Open Dashboard" }, true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", if portuguese { "Configurações" } else { "Settings" }, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", if portuguese { "Sair do TempoAI" } else { "Quit TempoAI" }, true, None::<&str>)?;
    Menu::with_items(app, &[&open, &settings, &quit])
}

pub fn apply_desktop_language(app: &AppHandle, portuguese: bool) -> Result<(), &'static str> {
    *app.state::<DesktopState>().portuguese.lock().unwrap_or_else(|error| error.into_inner()) = Some(portuguese);
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let menu = tray_menu(app, portuguese).map_err(|_| "Could not update tray language.")?;
        tray.set_menu(Some(menu)).map_err(|_| "Could not update tray language.")?;
    }
    if let Some(quick) = app.get_webview_window("quick") {
        let _ = quick.set_title(if portuguese { "Monitor rápido do TempoAI" } else { "TempoAI Quick Monitor" });
    }
    Ok(())
}

#[tauri::command]
pub fn set_desktop_language(app: AppHandle, window: WebviewWindow, portuguese: bool) -> Result<(), String> {
    preferences::require_window(window.label(), &["main", "quick"], "Open TempoAI to change the language.")?;
    apply_desktop_language(&app, portuguese).map_err(str::to_owned)
}

#[tauri::command]
pub fn open_dashboard(app: AppHandle, settings: bool) { show_main(&app, settings); }

#[tauri::command]
pub fn hide_panel(app: AppHandle) {
    if let Some(window) = app.get_webview_window("quick") { let _ = window.hide(); }
}

fn schedule_quick_bounds_save(app: &AppHandle, bounds: QuickBounds) {
    let seq = app.state::<DesktopState>().quick_save_seq.fetch_add(1, Ordering::AcqRel) + 1;
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        if handle.state::<DesktopState>().quick_save_seq.load(Ordering::Acquire) == seq {
            preferences::persist_quick_bounds(&handle, bounds);
        }
    });
}

fn record_quick_geometry(window: &tauri::Window) {
    let (Ok(pos), Ok(size)) = (window.outer_position(), window.inner_size()) else { return; };
    if size.width < 260 || size.height < 220 {
        return;
    }
    let state = window.state::<DesktopState>();
    let mut position = state.quick_position.lock().unwrap_or_else(|error| error.into_inner());
    let bounds = QuickBounds { x: pos.x, y: pos.y, width: size.width, height: size.height };
    position.saved = Some(bounds);
    position.placed = None;
    drop(position);
    schedule_quick_bounds_save(window.app_handle(), bounds);
}

pub fn window_event(window: &tauri::Window, event: &tauri::WindowEvent) {
    let config = window.state::<DesktopState>();
    let (minimize_to_tray, pin_panel) = {
        let config = config.config.lock().unwrap_or_else(|error| error.into_inner());
        (config.minimize_to_tray, config.pin_panel)
    };
    match event {
        tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "quick" || minimize_to_tray => {
            api.prevent_close();
            let _ = window.hide();
        }
        tauri::WindowEvent::CloseRequested { api, .. } if window.label() == "main" => {
            api.prevent_close();
            let app = window.app_handle();
            let _ = app.global_shortcut().unregister_all();
            app.exit(0);
        }
        tauri::WindowEvent::Focused(false) if window.label() == "quick" && !pin_panel => { let _ = window.hide(); }
        tauri::WindowEvent::Moved(point) if window.label() == "quick" && window.is_visible().unwrap_or(false) => {
            let placed = config.quick_position.lock().unwrap_or_else(|error| error.into_inner()).placed;
            if placed != Some(*point) {
                record_quick_geometry(window);
            }
        }
        tauri::WindowEvent::Resized(_) if window.label() == "quick" && window.is_visible().unwrap_or(false) => {
            record_quick_geometry(window);
        }
        tauri::WindowEvent::Focused(true) if window.label() == "main" => monitor::queue(window.app_handle(), true),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opening_dashboard_keeps_quick_monitor_open_when_pinned() {
        assert!(!should_hide_quick_on_dashboard(true));
        assert!(should_hide_quick_on_dashboard(false));
    }
}
