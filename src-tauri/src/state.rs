use crate::{
    error::AppResult,
    manifest::{Owner, PackManifest},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstanceState {
    pub pack_id: String,
    pub installed_version: String,
    pub last_manifest_sha256: String,
    pub managed_files: BTreeMap<String, ManagedFileState>,
    pub user_overrides: Vec<String>,
    pub backups: Vec<BackupRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedFileState {
    pub sha256: String,
    pub owner: Owner,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    pub created_at: String,
}

pub fn state_path(instance_dir: &Path) -> PathBuf {
    instance_dir.join(".packdelta").join("state.json")
}

pub fn read_state(instance_dir: &Path) -> AppResult<Option<InstanceState>> {
    let path = state_path(instance_dir);
    if !path.exists() {
        return Ok(None);
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

pub fn write_state(instance_dir: &Path, state: &InstanceState) -> AppResult<()> {
    let path = state_path(instance_dir);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(state)?)?;
    Ok(())
}

pub fn build_state(manifest: &PackManifest, manifest_sha256: String) -> InstanceState {
    let managed_files = manifest
        .files
        .iter()
        .filter(|file| file.owner == Owner::Pack)
        .map(|file| {
            (
                file.path.clone(),
                ManagedFileState {
                    sha256: file.sha256.clone(),
                    owner: Owner::Pack,
                    version: manifest.version.clone(),
                },
            )
        })
        .collect();

    InstanceState {
        pack_id: manifest.pack_id.clone(),
        installed_version: manifest.version.clone(),
        last_manifest_sha256: manifest_sha256,
        managed_files,
        user_overrides: Vec::new(),
        backups: Vec::new(),
    }
}

pub fn backup_record(id: String, from: String, to: String) -> BackupRecord {
    BackupRecord {
        id,
        from,
        to,
        created_at: Utc::now().to_rfc3339(),
    }
}
