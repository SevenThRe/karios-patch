use crate::{
    error::{AppError, AppResult, msg},
    hash::sha256_file,
};
use chrono::Utc;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Cursor, Write},
    path::{Path, PathBuf},
    process::Command,
};
use zip::ZipArchive;

pub const DEFAULT_UPDATE_SOURCE: &str =
    "https://github.com/SevenThRe/karios-patch/releases/latest/download/release-index.json";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateSourceConfig {
    pub index_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReleaseIndex {
    pub app_id: String,
    pub latest: String,
    pub releases: Vec<AppRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppRelease {
    pub version: String,
    pub notes: Option<String>,
    pub published_at: Option<String>,
    pub portable: PortableAsset,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableAsset {
    pub url: String,
    pub sha256: String,
    pub size: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppUpdateCheck {
    pub current_version: String,
    pub latest_version: String,
    pub update_available: bool,
    pub release: Option<AppRelease>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadedUpdate {
    pub version: String,
    pub archive_path: String,
    pub sha256: String,
    pub downloaded_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PortableInstallPlan {
    pub version: String,
    pub archive_path: String,
    pub staging_dir: String,
    pub script_path: String,
}

pub fn config_path() -> AppResult<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| AppError::Message("无法定位用户配置目录".to_string()))?
        .join("KairosPatch");
    Ok(base.join("update-source.json"))
}

pub fn load_source() -> AppResult<Option<UpdateSourceConfig>> {
    let path = config_path()?;
    if !path.exists() {
        return Ok(Some(UpdateSourceConfig {
            index_url: DEFAULT_UPDATE_SOURCE.to_string(),
        }));
    }
    Ok(Some(serde_json::from_slice(&fs::read(path)?)?))
}

pub fn save_source(config: UpdateSourceConfig) -> AppResult<UpdateSourceConfig> {
    validate_url(&config.index_url)?;
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&config)?)?;
    Ok(config)
}

pub fn check(index_url: &str, current_version: &str) -> AppResult<AppUpdateCheck> {
    validate_url(index_url)?;
    let index: ReleaseIndex = reqwest::blocking::get(index_url)?.error_for_status()?.json()?;
    let latest = index
        .releases
        .iter()
        .find(|release| release.version == index.latest)
        .cloned()
        .or_else(|| index.releases.first().cloned());
    let latest_version = latest
        .as_ref()
        .map(|release| release.version.clone())
        .unwrap_or(index.latest);

    let update_available = is_newer(&latest_version, current_version);
    Ok(AppUpdateCheck {
        current_version: current_version.to_string(),
        latest_version,
        update_available,
        release: update_available.then_some(latest).flatten(),
    })
}

pub fn download(release: AppRelease) -> AppResult<DownloadedUpdate> {
    validate_url(&release.portable.url)?;
    let bytes = reqwest::blocking::get(&release.portable.url)?
        .error_for_status()?
        .bytes()?;
    let cache = update_cache_dir()?.join(&release.version);
    fs::create_dir_all(&cache)?;
    let archive_path = cache.join("kairos-patch-portable.zip");
    fs::write(&archive_path, &bytes)?;

    let actual_sha = sha256_file(&archive_path)?;
    if !actual_sha.eq_ignore_ascii_case(&release.portable.sha256) {
        return Err(AppError::Message(format!(
            "portable zip SHA256 校验失败，期望 {}，实际 {}",
            release.portable.sha256, actual_sha
        )));
    }

    Ok(DownloadedUpdate {
        version: release.version,
        archive_path: archive_path.display().to_string(),
        sha256: actual_sha,
        downloaded_at: Utc::now().to_rfc3339(),
    })
}

pub fn prepare_portable_install(app: tauri::AppHandle, downloaded: DownloadedUpdate) -> AppResult<PortableInstallPlan> {
    let exe_path = std::env::current_exe()?;
    let app_dir = exe_path
        .parent()
        .ok_or_else(|| AppError::Message("无法定位当前程序目录".to_string()))?
        .to_path_buf();
    let archive_path = PathBuf::from(&downloaded.archive_path);
    if !archive_path.exists() {
        return msg("更新包不存在，请重新下载");
    }

    let staging_dir = update_cache_dir()?.join(&downloaded.version).join("staged");
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)?;
    }
    fs::create_dir_all(&staging_dir)?;
    extract_zip(&archive_path, &staging_dir)?;

    let script_path = update_cache_dir()?
        .join(&downloaded.version)
        .join("apply-portable-update.ps1");
    write_install_script(&script_path, &staging_dir, &app_dir, &exe_path)?;

    Command::new("powershell")
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .arg("-Pid")
        .arg(std::process::id().to_string())
        .spawn()?;

    app.exit(0);

    Ok(PortableInstallPlan {
        version: downloaded.version,
        archive_path: downloaded.archive_path,
        staging_dir: staging_dir.display().to_string(),
        script_path: script_path.display().to_string(),
    })
}

fn update_cache_dir() -> AppResult<PathBuf> {
    let base = dirs::cache_dir()
        .ok_or_else(|| AppError::Message("无法定位用户缓存目录".to_string()))?
        .join("KairosPatch")
        .join("updates");
    fs::create_dir_all(&base)?;
    Ok(base)
}

fn validate_url(url: &str) -> AppResult<()> {
    if url.starts_with("https://") {
        Ok(())
    } else {
        msg("更新源必须使用 https:// URL")
    }
}

fn is_newer(candidate: &str, current: &str) -> bool {
    let candidate = candidate.trim_start_matches('v');
    let current = current.trim_start_matches('v');
    match (Version::parse(candidate), Version::parse(current)) {
        (Ok(candidate), Ok(current)) => candidate > current,
        _ => candidate != current,
    }
}

fn extract_zip(archive_path: &Path, target_dir: &Path) -> AppResult<()> {
    let bytes = fs::read(archive_path)?;
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|error| AppError::Message(error.to_string()))?;

    for index in 0..archive.len() {
        let mut file = archive
            .by_index(index)
            .map_err(|error| AppError::Message(error.to_string()))?;
        let Some(path) = file.enclosed_name().map(|path| path.to_path_buf()) else {
            return Err(AppError::UnsafePath(file.name().to_string()));
        };
        let out = target_dir.join(path);
        if file.is_dir() {
            fs::create_dir_all(&out)?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut output = fs::File::create(out)?;
            std::io::copy(&mut file, &mut output)?;
        }
    }
    Ok(())
}

fn write_install_script(script_path: &Path, staging_dir: &Path, app_dir: &Path, exe_path: &Path) -> AppResult<()> {
    if let Some(parent) = script_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let script = format!(
        r#"
param([int]$Pid)
$ErrorActionPreference = "Stop"
$staging = "{staging}"
$appDir = "{app_dir}"
$exe = "{exe}"
if ($Pid -gt 0) {{
  Wait-Process -Id $Pid -ErrorAction SilentlyContinue
}}
Start-Sleep -Milliseconds 500
Copy-Item -Path (Join-Path $staging '*') -Destination $appDir -Recurse -Force
Start-Process -FilePath $exe -WorkingDirectory $appDir
"#,
        staging = escape_ps(staging_dir),
        app_dir = escape_ps(app_dir),
        exe = escape_ps(exe_path),
    );
    let mut file = fs::File::create(script_path)?;
    file.write_all(script.as_bytes())?;
    Ok(())
}

fn escape_ps(path: &Path) -> String {
    path.display().to_string().replace('"', "`\"")
}
