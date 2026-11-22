#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod core;
mod providers;

use core::{desktop, monitor, preferences, updates};
use providers::discovery;
use std::time::Duration;
use tauri::Manager;
use tauri_plugin_global_shortcut::ShortcutState;

fn main() {
    let test_directory = if cfg!(debug_assertions) {
        std::env::var_os("TEMPOAI_TEST_DATA_DIR").map(std::path::PathBuf::from).filter(|path| path.is_absolute())
    } else { None };
    let isolated = test_directory.is_some();
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_autostart::Builder::new().args(["--autostart"]).build())
        .plugin(tauri_plugin_notification::init());
    let builder = if isolated { builder } else {
        builder.plugin(tauri_plugin_single_instance::init(|app, _, _| desktop::show_main(app, false)))
            .plugin(tauri_plugin_window_state::Builder::default().with_denylist(&["quick"])
            .with_state_flags(tauri_plugin_window_state::StateFlags::SIZE | tauri_plugin_window_state::StateFlags::POSITION | tauri_plugin_window_state::StateFlags::MAXIMIZED).build())
    };
    builder
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, _, event| {
            if event.state() == ShortcutState::Pressed { desktop::toggle_quick(app, None); }
        }).build())
        .manage(preferences::DesktopState::default())
        .setup(move |app| {
            let directory = match test_directory {
                Some(path) => path,
                None => app.path().app_local_data_dir()?,
            };
            std::fs::create_dir_all(&directory)?;
            app.manage(monitor::Monitor::new(directory.join("history.sqlite")));
            // Load preferences & UI state first so the tray menu and Quick Monitor boot with saved language and bounds.
            preferences::initialize(app.handle())?;
            desktop::initialize(app.handle())?;
            let open_panel = app.state::<preferences::DesktopState>().config.lock().unwrap_or_else(|e| e.into_inner()).open_panel_at_startup;
            let autostart = std::env::args_os().any(|arg| arg == "--autostart");
            if open_panel { desktop::toggle_quick(app.handle(), None); }
            else if !autostart { desktop::show_main(app.handle(), false); }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut tick: u8 = 0;
                loop {
                    if tick == 0 {
                        monitor::queue(&handle, false);
                    } else {
                        monitor::poll_fast_watches(&handle);
                    }
                    tick = (tick + 1) % 8;
                    std::thread::sleep(Duration::from_secs(2));
                }
            });
            Ok(())
        })
        .on_window_event(desktop::window_event)
        .invoke_handler(tauri::generate_handler![
            preferences::get_preferences, preferences::save_preferences, preferences::connect_provider, preferences::remove_provider, preferences::set_pin,
            preferences::get_ui_state, preferences::save_ui_state,
            desktop::open_dashboard, desktop::hide_panel, desktop::set_desktop_language,
            discovery::scan_providers,
            monitor::usage_snapshot, monitor::session_history, monitor::request_refresh, monitor::refresh_claude, monitor::refresh_antigravity, monitor::test_saved_integrations, monitor::data_location, monitor::clear_history,
            updates::check_for_updates, updates::open_release_url,
        ])
        .run(tauri::generate_context!())
        .expect("failed to start TempoAI");
}
