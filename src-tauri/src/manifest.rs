use crate::{
    error::{AppError, AppResult},
    hash::{sha256_file, sha256_reader},
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock, mpsc},
    thread,
    time::SystemTime,
};
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackManifest {
    pub schema_version: u32,
    pub pack_id: String,
    pub pack_name: String,
    pub version: String,
    pub source_kind: SourceKind,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    CompletePack,
    CurseForgeManifestOnly,
    ModrinthManifestOnly,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ManifestScanProgress {
    pub source: String,
    pub current: usize,
    pub total: usize,
    pub path: Option<String>,
}

#[derive(Debug, Clone)]
struct ZipSourceIndex {
    byte_len: u64,
    modified_at: Option<SystemTime>,
    entries: BTreeMap<String, String>,
}

static ZIP_SOURCE_INDEX_CACHE: OnceLock<Mutex<BTreeMap<PathBuf, ZipSourceIndex>>> = OnceLock::new();
const MAX_INDEX_MANIFEST_BYTES: usize = 8 * 1024 * 1024;

pub fn scan_pack_source(
    root: &Path,
    pack_id: Option<String>,
    pack_name: Option<String>,
    version: Option<String>,
) -> AppResult<PackManifest> {
    scan_pack_source_with_progress(root, pack_id, pack_name, version, None)
}

pub fn scan_pack_source_with_progress(
    root: &Path,
    pack_id: Option<String>,
    pack_name: Option<String>,
    version: Option<String>,
    mut on_progress: Option<&mut dyn FnMut(ManifestScanProgress)>,
) -> AppResult<PackManifest> {
    if is_zip_source(root) {
        return scan_zip_pack_source(root, pack_id, pack_name, version, on_progress);
    }

    let source = root.display().to_string();
    let source_name = root
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("minecraft-pack")
        .to_string();
    let mut candidates = Vec::new();

    for entry in WalkDir::new(root)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = normalize_source_relative(root, path)?;
        if should_skip(&rel) {
            continue;
        }

        candidates.push((path.to_path_buf(), rel));
    }

    let total = candidates.len();
    let mut files = if on_progress.is_none() && total >= 32 {
        hash_directory_candidates_parallel(&candidates, &source_name)?
    } else {
        hash_directory_candidates_serial(candidates, &source_name, &source, &mut on_progress)?
    };
    report_manifest_progress(&mut on_progress, &source, total, total, None);

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let fallback_name = source_name;
    let source_kind = detect_source_kind(root, &files);

    Ok(PackManifest {
        schema_version: 1,
        pack_id: pack_id.unwrap_or_else(|| slugify(&fallback_name)),
        pack_name: pack_name.unwrap_or(fallback_name),
        version: version.unwrap_or_else(|| "local".to_string()),
        source_kind,
        mc_version: None,
        loader: None,
        created_at: Utc::now().to_rfc3339(),
        files,
    })
}

fn hash_directory_candidates_serial(
    candidates: Vec<(PathBuf, String)>,
    source_name: &str,
    source: &str,
    on_progress: &mut Option<&mut dyn FnMut(ManifestScanProgress)>,
) -> AppResult<Vec<ManifestFile>> {
    let total = candidates.len();
    let mut files = Vec::with_capacity(total);
    for (index, (path, rel)) in candidates.into_iter().enumerate() {
        report_manifest_progress(on_progress, source, index, total, Some(rel.clone()));
        files.push(manifest_file_from_path(&path, rel, source_name)?);
    }
    Ok(files)
}

fn hash_directory_candidates_parallel(
    candidates: &[(PathBuf, String)],
    source_name: &str,
) -> AppResult<Vec<ManifestFile>> {
    let worker_count = thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(1)
        .clamp(1, 8)
        .min(candidates.len());
    if worker_count <= 1 {
        return candidates
            .iter()
            .map(|(path, rel)| manifest_file_from_path(path, rel.clone(), source_name))
            .collect();
    }

    let chunk_size = candidates.len().div_ceil(worker_count);
    let (sender, receiver) = mpsc::channel();
    thread::scope(|scope| {
        for chunk in candidates.chunks(chunk_size) {
            let sender = sender.clone();
            scope.spawn(move || {
                let mut files = Vec::with_capacity(chunk.len());
                for (path, rel) in chunk {
                    match manifest_file_from_path(path, rel.clone(), source_name) {
                        Ok(file) => files.push(file),
                        Err(error) => {
                            let _ = sender.send(Err(error));
                            return;
                        }
                    }
                }
                let _ = sender.send(Ok(files));
            });
        }
    });
    drop(sender);

    let mut files = Vec::with_capacity(candidates.len());
    for result in receiver {
        files.extend(result?);
    }
    Ok(files)
}

fn manifest_file_from_path(
    path: &Path,
    relative_path: String,
    source_name: &str,
) -> AppResult<ManifestFile> {
    let metadata = fs::metadata(path)?;
    let (owner, strategy, file_type) = classify_for_source(&relative_path, source_name);
    Ok(ManifestFile {
        path: relative_path,
        sha256: sha256_file(path)?,
        size: metadata.len(),
        owner,
        strategy,
        file_type,
    })
}

pub fn read_source_file_prefix(
    source: &Path,
    relative_path: &str,
    max_bytes: usize,
) -> AppResult<Vec<u8>> {
    let mut content = Vec::with_capacity(max_bytes.saturating_add(1).min(64 * 1024));
    let limit = max_bytes.saturating_add(1) as u64;
    if is_zip_source(source) {
        return with_zip_source_entry(source, relative_path, |entry| {
            entry.by_ref().take(limit).read_to_end(&mut content)?;
            Ok(content)
        });
    }
    let mut file = File::open(find_source_file_path(source, relative_path)?)?;
    std::io::Read::by_ref(&mut file)
        .take(limit)
        .read_to_end(&mut content)?;
    Ok(content)
}

pub fn copy_source_file(source: &Path, relative_path: &str, destination: &Path) -> AppResult<()> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent)?;
    }
    if is_zip_source(source) {
        return with_zip_source_entry(source, relative_path, |entry| {
            let mut output = File::create(destination)?;
            std::io::copy(entry, &mut output)?;
            Ok(())
        });
    }
    fs::copy(find_source_file_path(source, relative_path)?, destination)?;
    Ok(())
}

pub fn copy_source_file_verified(
    source: &Path,
    relative_path: &str,
    destination: &Path,
    expected_sha: Option<&str>,
) -> AppResult<()> {
    let temp_path = temporary_destination_path(destination)?;
    if let Err(error) = copy_source_file(source, relative_path, &temp_path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error);
    }

    if let Some(expected) = expected_sha {
        let actual_sha = sha256_file(&temp_path)?;
        if actual_sha != expected {
            let _ = fs::remove_file(&temp_path);
            return Err(AppError::Message(format!(
                "SHA256 check failed: {relative_path}"
            )));
        }
    }

    if destination.exists() {
        fs::remove_file(destination)?;
    }
    fs::rename(temp_path, destination)?;
    Ok(())
}

pub fn source_file_sha256(source: &Path, relative_path: &str) -> AppResult<String> {
    if is_zip_source(source) {
        return with_zip_source_entry(source, relative_path, |entry| sha256_reader(entry));
    }
    sha256_file(&find_source_file_path(source, relative_path)?)
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

fn normalize_source_relative(root: &Path, path: &Path) -> AppResult<String> {
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
    normalize_pack_parts(&parts).ok_or_else(|| AppError::UnsafePath(path.display().to_string()))
}

fn scan_zip_pack_source(
    zip_path: &Path,
    pack_id: Option<String>,
    pack_name: Option<String>,
    version: Option<String>,
    mut on_progress: Option<&mut dyn FnMut(ManifestScanProgress)>,
) -> AppResult<PackManifest> {
    let source = zip_path.display().to_string();
    let file = File::open(zip_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| AppError::Message(error.to_string()))?;
    let mut files = Vec::new();
    let total = archive.len();
    let fallback_name = zip_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("minecraft-pack")
        .to_string();

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
        report_manifest_progress(&mut on_progress, &source, index, total, Some(rel.clone()));
        let size = entry.size();
        let sha256 = sha256_reader(&mut entry)?;
        let (owner, strategy, file_type) = classify_for_source(&rel, &fallback_name);
        files.push(ManifestFile {
            path: rel,
            sha256,
            size,
            owner,
            strategy,
            file_type,
        });
    }
    report_manifest_progress(&mut on_progress, &source, total, total, None);

    files.sort_by(|a, b| a.path.cmp(&b.path));
    let source_kind = detect_source_kind(zip_path, &files);
    Ok(PackManifest {
        schema_version: 1,
        pack_id: pack_id.unwrap_or_else(|| slugify(&fallback_name)),
        pack_name: pack_name.unwrap_or(fallback_name),
        version: version.unwrap_or_else(|| "local".to_string()),
        source_kind,
        mc_version: None,
        loader: None,
        created_at: Utc::now().to_rfc3339(),
        files,
    })
}

fn with_zip_source_entry<T>(
    zip_path: &Path,
    relative_path: &str,
    read: impl FnOnce(&mut zip::read::ZipFile<'_, File>) -> AppResult<T>,
) -> AppResult<T> {
    if Path::new(relative_path).is_absolute() || relative_path.contains("..") {
        return Err(AppError::UnsafePath(relative_path.to_string()));
    }
    let index = zip_source_index(zip_path)?;
    let Some(entry_name) = index.entries.get(relative_path) else {
        return Err(AppError::Message(format!(
            "ZIP 内找不到源文件: {} -> {}",
            zip_path.display(),
            relative_path
        )));
    };
    let file = File::open(zip_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| AppError::Message(error.to_string()))?;
    let mut entry = archive
        .by_name(entry_name)
        .map_err(|error| AppError::Message(error.to_string()))?;
    read(&mut entry)
}

fn zip_source_index(zip_path: &Path) -> AppResult<ZipSourceIndex> {
    let metadata = fs::metadata(zip_path)?;
    let byte_len = metadata.len();
    let modified_at = metadata.modified().ok();
    let cache_key = zip_path
        .canonicalize()
        .unwrap_or_else(|_| zip_path.to_path_buf());
    let cache = ZIP_SOURCE_INDEX_CACHE.get_or_init(|| Mutex::new(BTreeMap::new()));

    {
        let cached = cache
            .lock()
            .map_err(|_| AppError::Message("ZIP source index cache is unavailable".to_string()))?;
        if let Some(index) = cached.get(&cache_key) {
            if index.byte_len == byte_len && index.modified_at == modified_at {
                return Ok(index.clone());
            }
        }
    }

    let index = build_zip_source_index(zip_path, byte_len, modified_at)?;
    let mut cached = cache
        .lock()
        .map_err(|_| AppError::Message("ZIP source index cache is unavailable".to_string()))?;
    cached.insert(cache_key, index.clone());
    Ok(index)
}

fn build_zip_source_index(
    zip_path: &Path,
    byte_len: u64,
    modified_at: Option<SystemTime>,
) -> AppResult<ZipSourceIndex> {
    let file = File::open(zip_path)?;
    let mut archive =
        ZipArchive::new(file).map_err(|error| AppError::Message(error.to_string()))?;
    let mut entries = BTreeMap::new();

    for index in 0..archive.len() {
        let entry = archive
            .by_index(index)
            .map_err(|error| AppError::Message(error.to_string()))?;
        if entry.is_dir() {
            continue;
        }
        if let Some(relative) = normalize_zip_entry_name(entry.name()) {
            entries
                .entry(relative)
                .or_insert_with(|| entry.name().to_string());
        }
    }

    Ok(ZipSourceIndex {
        byte_len,
        modified_at,
        entries,
    })
}

fn find_source_file_path(source: &Path, relative_path: &str) -> AppResult<PathBuf> {
    let direct = resolve_safe(source, relative_path)?;
    if direct.exists() {
        return Ok(direct);
    }
    for entry in WalkDir::new(source)
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
    {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if normalize_source_relative(source, path)
            .is_ok_and(|normalized| normalized == relative_path)
        {
            return Ok(path.to_path_buf());
        }
    }
    Err(AppError::Message(format!(
        "源文件不存在: {} -> {}",
        source.display(),
        relative_path
    )))
}

fn temporary_destination_path(destination: &Path) -> AppResult<PathBuf> {
    let file_name = destination
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| AppError::UnsafePath(destination.display().to_string()))?;
    Ok(destination.with_file_name(format!("{file_name}.copying")))
}

fn report_manifest_progress(
    on_progress: &mut Option<&mut dyn FnMut(ManifestScanProgress)>,
    source: &str,
    current: usize,
    total: usize,
    path: Option<String>,
) {
    if let Some(callback) = on_progress.as_deref_mut() {
        callback(ManifestScanProgress {
            source: source.to_string(),
            current,
            total,
            path,
        });
    }
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

    normalize_pack_parts(&parts)
}

fn normalize_pack_parts(parts: &[String]) -> Option<String> {
    if parts.is_empty() {
        return None;
    }
    if parts[0].eq_ignore_ascii_case("overrides") && parts.len() > 1 {
        return Some(parts[1..].join("/"));
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
    if is_launcher_metadata_path(&p) {
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

fn classify_for_source(path: &str, source_name: &str) -> (Owner, Strategy, FileType) {
    if is_source_launcher_metadata_path(path, source_name) {
        return (Owner::Pack, Strategy::Replace, FileType::Other);
    }
    classify(path)
}

fn detect_source_kind(source: &Path, files: &[ManifestFile]) -> SourceKind {
    if files.iter().any(|file| file.file_type == FileType::Mod) {
        return SourceKind::CompletePack;
    }
    if files.iter().any(|file| file.path == "modrinth.index.json")
        && read_source_file_prefix(source, "modrinth.index.json", MAX_INDEX_MANIFEST_BYTES)
            .ok()
            .filter(|bytes| bytes.len() <= MAX_INDEX_MANIFEST_BYTES)
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|value| {
                value
                    .get("files")
                    .and_then(|files| files.as_array())
                    .cloned()
            })
            .is_some_and(|files| !files.is_empty())
    {
        return SourceKind::ModrinthManifestOnly;
    }
    if files.iter().any(|file| file.path == "manifest.json")
        && read_source_file_prefix(source, "manifest.json", MAX_INDEX_MANIFEST_BYTES)
            .ok()
            .filter(|bytes| bytes.len() <= MAX_INDEX_MANIFEST_BYTES)
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .is_some_and(|value| {
                value
                    .get("manifestType")
                    .and_then(|item| item.as_str())
                    .is_some_and(|item| item.eq_ignore_ascii_case("minecraftModpack"))
                    && value
                        .get("files")
                        .and_then(|items| items.as_array())
                        .is_some_and(|items| !items.is_empty())
            })
    {
        return SourceKind::CurseForgeManifestOnly;
    }
    SourceKind::Unknown
}

pub fn is_launcher_metadata_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    if p.contains('/') {
        return false;
    }
    matches!(
        p.as_str(),
        "manifest.json"
            | "mmc-pack.json"
            | "instance.cfg"
            | "minecraftinstance.json"
            | "pack.toml"
            | "modrinth.index.json"
    )
}

pub fn is_source_launcher_metadata_path(path: &str, source_name: &str) -> bool {
    if is_launcher_metadata_path(path) {
        return true;
    }
    let p = path.to_ascii_lowercase();
    if p.contains('/') || !p.ends_with(".json") {
        return false;
    }
    let Some(stem) = Path::new(path).file_stem().and_then(|value| value.to_str()) else {
        return false;
    };
    stem.eq_ignore_ascii_case(source_name)
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn detects_complete_pack_when_mod_jars_exist() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("Pack");
        write_file(&source, "mods/example.jar", b"jar");

        let manifest = scan_pack_source(&source, None, None, None).unwrap();

        assert_eq!(manifest.source_kind, SourceKind::CompletePack);
    }

    #[test]
    fn detects_curseforge_manifest_only_pack() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("CurseForgePack");
        write_file(
            &source,
            "manifest.json",
            br#"{"manifestType":"minecraftModpack","files":[{"projectID":1,"fileID":2}]}"#,
        );
        write_file(&source, "overrides/config/app.toml", b"enabled=true\n");

        let manifest = scan_pack_source(&source, None, None, None).unwrap();

        assert_eq!(manifest.source_kind, SourceKind::CurseForgeManifestOnly);
    }

    #[test]
    fn detects_modrinth_manifest_only_pack() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("ModrinthPack");
        write_file(
            &source,
            "modrinth.index.json",
            br#"{"formatVersion":1,"files":[{"path":"mods/example.jar","downloads":["https://example.test/example.jar"]}]}"#,
        );

        let manifest = scan_pack_source(&source, None, None, None).unwrap();

        assert_eq!(manifest.source_kind, SourceKind::ModrinthManifestOnly);
    }

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
}
