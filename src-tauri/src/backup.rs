use crate::{
    error::AppResult,
    manifest::resolve_safe,
    state::{InstanceState, state_path},
};
use chrono::Local;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupManifest {
    pub backup_id: String,
    pub from: String,
    pub to: String,
    pub files: Vec<BackupFile>,
    #[serde(default)]
    pub operation_files: Vec<BackupOperationFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupFile {
    pub path: String,
    pub sha256: String,
    pub backup_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupOperationFile {
    pub path: String,
    pub action: String,
    pub source_path: Option<String>,
}

pub fn make_backup_id(from: &str, to: &str) -> String {
    format!(
        "{}_{}_to_{}",
        Local::now().format("%Y-%m-%d_%H-%M-%S"),
        sanitize(from),
        sanitize(to)
    )
}

pub fn create_backup(
    instance_dir: &Path,
    backup_id: &str,
    from: &str,
    to: &str,
    files: Vec<BackupFile>,
    state_before: &InstanceState,
) -> AppResult<BackupManifest> {
    let backup_dir = instance_dir
        .join(".packdelta")
        .join("backups")
        .join(backup_id);
    fs::create_dir_all(backup_dir.join("files"))?;
    fs::write(
        backup_dir.join("state-before.json"),
        serde_json::to_vec_pretty(state_before)?,
    )?;

    let manifest = BackupManifest {
        backup_id: backup_id.to_string(),
        from: from.to_string(),
        to: to.to_string(),
        files,
        operation_files: Vec::new(),
    };
    fs::write(
        backup_dir.join("backup-manifest.json"),
        serde_json::to_vec_pretty(&manifest)?,
    )?;
    Ok(manifest)
}

pub fn write_operation_files(
    instance_dir: &Path,
    backup_id: &str,
    operation_files: Vec<BackupOperationFile>,
) -> AppResult<()> {
    let backup_dir = instance_dir
        .join(".packdelta")
        .join("backups")
        .join(backup_id);
    let path = backup_dir.join("backup-manifest.json");
    let mut manifest: BackupManifest = serde_json::from_slice(&fs::read(&path)?)?;
    manifest.operation_files = operation_files;
    fs::write(path, serde_json::to_vec_pretty(&manifest)?)?;
    Ok(())
}

pub fn copy_into_backup(
    instance_dir: &Path,
    backup_id: &str,
    relative_path: &str,
) -> AppResult<Option<BackupFile>> {
    let source = resolve_safe(instance_dir, relative_path)?;
    if !source.exists() {
        return Ok(None);
    }
    let backup_relative = format!("files/{}", relative_path);
    let destination = instance_dir
        .join(".packdelta")
        .join("backups")
        .join(backup_id)
        .join(backup_relative.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(&source, &destination)?;
    Ok(Some(BackupFile {
        path: relative_path.to_string(),
        sha256: crate::hash::sha256_file(&source)?,
        backup_path: backup_relative,
    }))
}

pub fn read_backup_manifest(instance_dir: &Path, backup_id: &str) -> AppResult<BackupManifest> {
    let path = instance_dir
        .join(".packdelta")
        .join("backups")
        .join(backup_id)
        .join("backup-manifest.json");
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn read_state_before(instance_dir: &Path, backup_id: &str) -> AppResult<InstanceState> {
    let path = instance_dir
        .join(".packdelta")
        .join("backups")
        .join(backup_id)
        .join("state-before.json");
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn write_rollback_safety(instance_dir: &Path, backup_id: &str) -> AppResult<()> {
    let current = state_path(instance_dir);
    if !current.exists() {
        return Ok(());
    }
    let target = instance_dir
        .join(".packdelta")
        .join("backups")
        .join(backup_id)
        .join("rollback-safety-state.json");
    fs::copy(current, target)?;
    Ok(())
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '.' || ch == '-' {
                ch
            } else {
                '_'
            }
        })
        .collect()
}
