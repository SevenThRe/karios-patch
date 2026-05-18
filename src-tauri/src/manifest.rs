use crate::{error::{AppError, AppResult}, hash::{sha256_bytes, sha256_file}};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Cursor, Read},
    path::{Component, Path, PathBuf},
};
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub schema_version: u32,
    pub pack_id: String,
    pub pack_name: String,
    pub version: String,
    pub mc_version: Option<String>,
    pub loader: Option<LoaderInfo>,
    pub created_at: String,
    pub files: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoaderInfo {
    #[serde(rename = "type")]
    pub loader_type: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub path: String,
    pub sha256: String,
    pub size: u64,
    pub owner: Owner,
    pub strategy: Strategy,
    #[serde(rename = "type")]
    pub file_type: FileType,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Owner {
    Pack,
    User,
    Runtime,
    Cache,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Strategy {
    Replace,
    Preserve,
    Merge,
    Ignore,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileType {
    Mod,
    Config,
    Script,
    ResourcePack,
    ShaderPack,
    Save,
    Runtime,
    Other,
}

pub fn scan_pack_source(
    root: &Path,
    pack_id: Option<String>,
    pack_name: Option<String>,
    version: Option<String>,
) -> AppResult<PackManifest> {
    if is_zip_source(root) {
        return scan_zip_pack_source(root, pack_id, pack_name, version);
    }

    let mut files = Vec::new();

    for entry in WalkDir::new(root).follow_links(false).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = normalize_relative(root, path)?;
        if should_skip(&rel) {
            continue;
        }

        let metadata = fs::metadata(path)?;
        let (owner, strategy, file_type) = classify(&rel);
        files.push(ManifestFile {
            path: rel,
            sha256: sha256_file(path)?,
            size: metadata.len(),
            owner,
            strategy,
            file_type,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let fallback_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("minecraft-pack")
        .to_string();

    Ok(PackManifest {
        schema_version: 1,
        pack_id: pack_id.unwrap_or_else(|| slugify(&fallback_name)),
        pack_name: pack_name.unwrap_or(fallback_name),
        version: version.unwrap_or_else(|| "local".to_string()),
        mc_version: None,
        loader: None,
        created_at: Utc::now().to_rfc3339(),
        files,
    })
}

pub fn read_source_file(source: &Path, relative_path: &str) -> AppResult<Vec<u8>> {
    if is_zip_source(source) {
        return read_zip_source_file(source, relative_path);
    }
    Ok(fs::read(resolve_safe(source, relative_path)?)?)
}

pub fn source_file_exists(source: &Path, relative_path: &str) -> AppResult<bool> {
    if is_zip_source(source) {
        return Ok(read_zip_source_file(source, relative_path).is_ok());
    }
    Ok(resolve_safe(source, relative_path)?.exists())
}

pub fn is_zip_source(source: &Path) -> bool {
    source.is_file()
        && source
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("zip"))
}

pub fn write_manifest(path: &Path, manifest: &PackManifest) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(manifest)?)?;
    Ok(())
}

fn normalize_relative(root: &Path, path: &Path) -> AppResult<String> {
    let rel = path
        .strip_prefix(root)
        .map_err(|_| crate::error::AppError::UnsafePath(path.display().to_string()))?;
    let parts: Vec<String> = rel
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().to_string()),
            _ => None,
        })
        .collect();
    Ok(parts.join("/"))
}

fn scan_zip_pack_source(
    zip_path: &Path,
    pack_id: Option<String>,
    pack_name: Option<String>,
    version: Option<String>,
) -> AppResult<PackManifest> {
    let bytes = fs::read(zip_path)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::Message(error.to_string()))?;
    let mut files = Vec::new();

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::Message(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = normalize_zip_entry_name(entry.name()) else {
            continue;
        };
        if should_skip(&rel) {
            continue;
        }
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        let (owner, strategy, file_type) = classify(&rel);
        files.push(ManifestFile {
            path: rel,
            sha256: sha256_bytes(&content),
            size: content.len() as u64,
            owner,
            strategy,
            file_type,
        });
    }

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let fallback_name = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("minecraft-pack")
        .to_string();

    Ok(PackManifest {
        schema_version: 1,
        pack_id: pack_id.unwrap_or_else(|| slugify(&fallback_name)),
        pack_name: pack_name.unwrap_or(fallback_name),
        version: version.unwrap_or_else(|| "local".to_string()),
        mc_version: None,
        loader: None,
        created_at: Utc::now().to_rfc3339(),
        files,
    })
}

fn read_zip_source_file(zip_path: &Path, relative_path: &str) -> AppResult<Vec<u8>> {
    if Path::new(relative_path).is_absolute() || relative_path.contains("..") {
        return Err(AppError::UnsafePath(relative_path.to_string()));
    }
    let bytes = fs::read(zip_path)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::Message(error.to_string()))?;

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| AppError::Message(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if normalize_zip_entry_name(entry.name()).as_deref() != Some(relative_path) {
            continue;
        }
        let mut content = Vec::new();
        entry.read_to_end(&mut content)?;
        return Ok(content);
    }

    Err(AppError::Message(format!(
        "ZIP 内找不到源文件: {} -> {}",
        zip_path.display(),
        relative_path
    )))
}

fn normalize_zip_entry_name(name: &str) -> Option<String> {
    let raw_path = Path::new(name);
    if raw_path.is_absolute() || name.contains("..") {
        return None;
    }
    let parts: Vec<String> = raw_path
        .components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().replace('\\', "/")),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return None;
    }

    let start = parts
        .iter()
        .position(|part| is_known_pack_root(part))
        .unwrap_or(0);
    Some(parts[start..].join("/"))
}

fn is_known_pack_root(part: &str) -> bool {
    matches!(
        part.to_ascii_lowercase().as_str(),
        "mods"
            | "defaultconfigs"
            | "kubejs"
            | "scripts"
            | "libraries"
            | "packmenu"
            | "config"
            | "resourcepacks"
            | "shaderpacks"
            | "saves"
            | "logs"
            | "crash-reports"
            | "screenshots"
            | "journeymap"
            | "xaero"
    ) || matches!(
        part.to_ascii_lowercase().as_str(),
        "options.txt" | "optionsof.txt" | "servers.dat"
    )
}

pub fn resolve_safe(root: &Path, relative: &str) -> AppResult<PathBuf> {
    let rel = Path::new(relative);
    if rel.is_absolute() || relative.contains("..") {
        return Err(crate::error::AppError::UnsafePath(relative.to_string()));
    }
    Ok(root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR)))
}

fn should_skip(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.starts_with(".packdelta/")
        || p.starts_with("logs/")
        || p.starts_with("crash-reports/")
        || p.starts_with("screenshots/")
        || p.starts_with("saves/")
        || p.ends_with("/thumbs.db")
}

fn classify(path: &str) -> (Owner, Strategy, FileType) {
    let p = path.to_ascii_lowercase();
    if p.starts_with("mods/") && p.ends_with(".jar") {
        return (Owner::Pack, Strategy::Replace, FileType::Mod);
    }
    if p.starts_with("defaultconfigs/") {
        return (Owner::Pack, Strategy::Replace, FileType::Config);
    }
    if p.starts_with("kubejs/startup_scripts/")
        || p.starts_with("kubejs/server_scripts/")
        || p.starts_with("kubejs/client_scripts/")
        || p.starts_with("scripts/")
    {
        return (Owner::Pack, Strategy::Replace, FileType::Script);
    }
    if p.starts_with("libraries/") || p.starts_with("packmenu/") {
        return (Owner::Pack, Strategy::Replace, FileType::Other);
    }
    if p.starts_with("config/") {
        return (Owner::User, Strategy::Merge, FileType::Config);
    }
    if p.starts_with("resourcepacks/") {
        return (Owner::User, Strategy::Preserve, FileType::ResourcePack);
    }
    if p.starts_with("shaderpacks/") {
        return (Owner::User, Strategy::Preserve, FileType::ShaderPack);
    }
    if matches!(p.as_str(), "options.txt" | "optionsof.txt" | "servers.dat")
        || p.starts_with("journeymap/")
        || p.starts_with("xaero/")
    {
        return (Owner::User, Strategy::Preserve, FileType::Other);
    }
    (Owner::Ignored, Strategy::Ignore, FileType::Other)
}

fn slugify(value: &str) -> String {
    let mut out = String::new();
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_matches('-').to_string()
}
