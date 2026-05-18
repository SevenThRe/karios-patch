use crate::{
    backup,
    diff::{self, ManifestDiff},
    error::AppResult,
    manifest::{PackManifest, scan_pack_source as scan_source},
    patch::{self, ApplyResult, RollbackResult, UpdatePlan},
    updater::{self, AppRelease, AppUpdateCheck, DownloadedUpdate, PortableInstallPlan, UpdateSourceConfig},
};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanOptions {
    pub pack_id: Option<String>,
    pub pack_name: Option<String>,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareResult {
    pub old_manifest: PackManifest,
    pub new_manifest: PackManifest,
    pub diff: ManifestDiff,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupSummary {
    pub id: String,
    pub from: String,
    pub to: String,
    pub file_count: usize,
}

#[tauri::command]
pub fn scan_pack_source(path: String, options: Option<ScanOptions>) -> AppResult<PackManifest> {
    let options = options.unwrap_or(ScanOptions {
        pack_id: None,
        pack_name: None,
        version: None,
    });
    scan_source(
        Path::new(&path),
        options.pack_id,
        options.pack_name,
        options.version,
    )
}

#[tauri::command]
pub fn compare_pack_sources(old_source: String, new_source: String) -> AppResult<CompareResult> {
    let old_manifest = scan_source(Path::new(&old_source), None, None, Some("old-local".to_string()))?;
    let new_manifest = scan_source(Path::new(&new_source), None, None, Some("new-local".to_string()))?;
    let diff = diff::compare(&old_manifest, &new_manifest);
    Ok(CompareResult {
        old_manifest,
        new_manifest,
        diff,
    })
}

#[tauri::command]
pub fn preview_update(instance_dir: String, old_source: String, new_source: String) -> AppResult<UpdatePlan> {
    let old_manifest = scan_source(Path::new(&old_source), None, None, Some("old-local".to_string()))?;
    let new_manifest = scan_source(Path::new(&new_source), None, None, Some("new-local".to_string()))?;
    let diff = diff::compare(&old_manifest, &new_manifest);
    patch::build_plan(Path::new(&instance_dir), &old_manifest, &new_manifest, &diff)
}

#[tauri::command]
pub fn apply_update(instance_dir: String, old_source: String, new_source: String) -> AppResult<ApplyResult> {
    let old_manifest = scan_source(Path::new(&old_source), None, None, Some("old-local".to_string()))?;
    let new_manifest = scan_source(Path::new(&new_source), None, None, Some("new-local".to_string()))?;
    patch::apply_update(
        Path::new(&instance_dir),
        Path::new(&old_source),
        Path::new(&new_source),
        old_manifest,
        new_manifest,
    )
}

#[tauri::command]
pub fn list_backups(instance_dir: String) -> AppResult<Vec<BackupSummary>> {
    let root = Path::new(&instance_dir).join(".packdelta").join("backups");
    if !root.exists() {
        return Ok(Vec::new());
    }
    let mut backups = Vec::new();
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        if !entry.path().is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let manifest = backup::read_backup_manifest(Path::new(&instance_dir), &id)?;
        backups.push(BackupSummary {
            id,
            from: manifest.from,
            to: manifest.to,
            file_count: manifest.files.len(),
        });
    }
    backups.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(backups)
}

#[tauri::command]
pub fn rollback(instance_dir: String, backup_id: String) -> AppResult<RollbackResult> {
    patch::rollback(Path::new(&instance_dir), &backup_id)
}

#[tauri::command]
pub fn open_folder(path: String) -> AppResult<()> {
    tauri_plugin_opener::open_path(path, None::<String>)
        .map_err(|error| crate::error::AppError::Message(error.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn load_update_source() -> AppResult<Option<UpdateSourceConfig>> {
    updater::load_source()
}

#[tauri::command]
pub fn save_update_source(index_url: String) -> AppResult<UpdateSourceConfig> {
    updater::save_source(UpdateSourceConfig { index_url })
}

#[tauri::command]
pub fn check_app_update(index_url: String) -> AppResult<AppUpdateCheck> {
    updater::check(&index_url, env!("CARGO_PKG_VERSION"))
}

#[tauri::command]
pub fn download_app_update(release: AppRelease) -> AppResult<DownloadedUpdate> {
    updater::download(release)
}

#[tauri::command]
pub fn install_portable_update(
    app: tauri::AppHandle,
    downloaded: DownloadedUpdate,
) -> AppResult<PortableInstallPlan> {
    updater::prepare_portable_install(app, downloaded)
}
