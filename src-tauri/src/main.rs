#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

mod backup;
mod commands;
mod diagnostics;
mod diff;
mod error;
mod hash;
mod instance;
mod manifest;
mod patch;
mod preferences;
mod state;
mod updater;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::scan_pack_source,
            commands::compare_pack_sources,
            commands::read_source_diff,
            commands::preview_conservative_update,
            commands::apply_conservative_update,
            commands::apply_conservative_update_tracked,
            commands::preview_update,
            commands::apply_update,
            commands::apply_update_tracked,
            commands::list_backups,
            commands::get_backup_detail,
            commands::rollback,
            commands::open_folder,
            commands::load_app_preferences,
            commands::save_app_preferences,
            commands::load_update_source,
            commands::save_update_source,
            commands::check_app_update,
            commands::fetch_changelog,
            commands::download_app_update,
            commands::install_portable_update,
            commands::append_app_log,
            commands::create_feedback_package
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Kairos Patch");
}
