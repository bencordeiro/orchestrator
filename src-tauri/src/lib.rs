//! Tauri application library: hosts MCP server + slot board GUI + CLIProxyAPI sidecar.

mod commands;
mod notify_bridge;
mod sidecar;
mod state;

use std::sync::Arc;

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, RunEvent, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

use state::{default_config_path, AppState};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let config_path = default_config_path();

    // Rotating log file (daily, capped at 7 files) under the app config dir, in
    // addition to stderr. Installed apps have no visible console, so this file is
    // the only way to diagnose a bad launch. Building it must never itself fail
    // startup — fall back to stderr-only if the file can't be opened.
    let log_dir = state::log_dir(&config_path);
    let _ = std::fs::create_dir_all(&log_dir);
    let (file_layer, _log_guard) = match tracing_appender::rolling::RollingFileAppender::builder()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .filename_prefix("orchestrator")
        .filename_suffix("log")
        .max_log_files(7)
        .build(&log_dir)
    {
        Ok(appender) => {
            let (writer, guard) = tracing_appender::non_blocking(appender);
            (
                Some(
                    tracing_subscriber::fmt::layer()
                        .with_ansi(false)
                        .with_writer(writer),
                ),
                Some(guard),
            )
        }
        Err(_) => (None, None),
    };

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "orchestrator=info,orchestrator_app=info,rmcp=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .with(file_layer)
        .init();

    tracing::info!(
        "Orchestrator v{} starting; slots config: {}; logs: {}",
        env!("CARGO_PKG_VERSION"),
        config_path.display(),
        log_dir.display()
    );

    // Bootstrap orchestrator + MCP on a multi-thread runtime before Tauri starts.
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    let app_state = rt
        .block_on(AppState::bootstrap(config_path))
        .expect("bootstrap orchestrator");

    // Keep the runtime alive for the MCP server task.
    let _rt_guard = Arc::new(rt);

    tauri::Builder::default()
        // Second launch focuses the existing window instead of silently dying
        // on the already-bound MCP port. MUST be the first plugin registered.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.show();
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![]),
        ))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .manage(app_state)
        .manage(_rt_guard)
        .invoke_handler(tauri::generate_handler![
            commands::get_server_info,
            commands::get_slot_board,
            commands::open_log_dir,
            commands::get_backend_profiles,
            commands::swap_slot_backend,
            commands::add_slot,
            commands::remove_slot,
            commands::update_slot_description,
            commands::upsert_backend_profile,
            commands::remove_backend_profile,
            commands::get_mcp_setup_command,
            commands::get_mcp_setup_commands,
            commands::set_secret,
            commands::get_sidecar_status,
            commands::set_sidecar_enabled,
            commands::list_subscription_accounts,
            commands::list_proxy_models,
            commands::set_account_model_override,
            commands::clear_account_model_override,
            commands::start_subscription_oauth,
            commands::disconnect_subscription_account,
            commands::sync_subscription_profiles,
            commands::list_oauth_providers,
            commands::set_slot_fallback,
            commands::discover_ollama_models,
            commands::get_ollama_extra_hosts,
            commands::set_ollama_extra_hosts,
            commands::create_ollama_profile,
            commands::get_recent_usage,
            commands::check_for_updates,
            commands::install_update,
        ])
        .setup(|app| {
            setup_tray(app.handle())?;
            // Wire tray notifications for worker-unavailable (MCP thread → GUI).
            if let Some(state) = app.try_state::<AppState>() {
                state.notify.set_app(app.handle().clone());
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Close-to-tray: hide instead of exit.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application")
        .run(|app_handle, event| {
            if let RunEvent::ExitRequested { api, .. } = event {
                // Clean shutdown of CLIProxyAPI sidecar.
                if let Some(state) = app_handle.try_state::<AppState>() {
                    let sidecar = state.sidecar.clone();
                    // Best-effort blocking kill; runtime may already be shutting down.
                    let _ = std::thread::spawn(move || {
                        let rt = tokio::runtime::Builder::new_current_thread()
                            .enable_all()
                            .build();
                        if let Ok(rt) = rt {
                            let _ = rt.block_on(sidecar.stop());
                        }
                    })
                    .join();
                }
                let _ = api;
            }
        });
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let hide_i = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &hide_i, &quit_i])?;

    let icon = app
        .default_window_icon()
        .cloned()
        .or_else(|| Image::from_bytes(include_bytes!("../icons/icon.png")).ok())
        .expect("app icon");

    let _tray = TrayIconBuilder::new()
        .icon(icon)
        .menu(&menu)
        .tooltip("Orchestrator")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            "hide" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.hide();
                }
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app = tray.app_handle();
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
        })
        .build(app)?;

    Ok(())
}
