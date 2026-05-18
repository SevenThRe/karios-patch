mod backup;
mod commands;
mod diff;
mod error;
mod hash;
mod manifest;
mod patch;
mod state;
mod updater;

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            commands::scan_pack_source,
            commands::compare_pack_sources,
            commands::preview_update,
            commands::apply_update,
            commands::list_backups,
            commands::rollback,
            commands::open_folder,
            commands::load_update_source,
            commands::save_update_source,
            commands::check_app_update,
            commands::download_app_update,
            commands::install_portable_update
        ])
        .run(tauri::generate_context!())
        .expect("failed to run Kairos Patch");
}
