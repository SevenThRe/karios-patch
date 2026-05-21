use crate::{
    error::{AppError, AppResult, msg},
    hash::sha256_file,
};
use chrono::Utc;
use semver::Version;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
};
use zip::ZipArchive;

const UPDATE_APP_ID: &str = "kairos-patch";
const TRUSTED_GITHUB_HOST: &str = "github.com";
const TRUSTED_GITHUB_OWNER: &str = "SevenThRe";
const TRUSTED_GITHUB_REPO: &str = "karios-patch";
const RELEASE_INDEX_ASSET: &str = "release-index.json";
const PORTABLE_ARCHIVE_NAME: &str = "kairos-patch-portable.zip";

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangelogRelease {
    pub version: String,
    pub title: String,
    pub body: String,
    pub published_at: Option<String>,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    published_at: Option<String>,
    html_url: String,
    draft: bool,
    prerelease: bool,
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
    validate_update_source_url(&config.index_url)?;
    let path = config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&config)?)?;
    Ok(config)
}

pub fn check(index_url: &str, current_version: &str) -> AppResult<AppUpdateCheck> {
    validate_update_source_url(index_url)?;
    let index: ReleaseIndex = reqwest::blocking::get(index_url)?
        .error_for_status()?
        .json()?;
    let latest_version = update_version_component(&index.latest)?;
    let releases = normalize_release_index(index)?;
    let latest = releases
        .into_iter()
        .find(|release| release.version == latest_version)
        .ok_or_else(|| AppError::Message("更新索引中找不到 latest 对应的 release".to_string()))?;

    let update_available = is_newer(&latest_version, current_version);
    Ok(AppUpdateCheck {
        current_version: current_version.to_string(),
        latest_version,
        update_available,
        release: update_available.then_some(latest),
    })
}

pub fn changelog(index_url: &str) -> AppResult<Vec<ChangelogRelease>> {
    validate_update_source_url(index_url)?;
    let (owner, repo) = github_repo_from_update_source(index_url)
        .ok_or_else(|| AppError::Message("无法从 GitHub 更新源识别仓库".to_string()))?;
    let url = format!("https://api.github.com/repos/{owner}/{repo}/releases?per_page=20");
    let client = reqwest::blocking::Client::new();
    let releases: Vec<GitHubRelease> = client
        .get(url)
        .header(reqwest::header::USER_AGENT, "KairosPatch")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json")
        .send()?
        .error_for_status()?
        .json()?;

    Ok(releases
        .into_iter()
        .filter(|release| !release.draft)
        .map(|release| ChangelogRelease {
            version: release.tag_name.clone(),
            title: release.name.unwrap_or(release.tag_name),
            body: release.body.unwrap_or_else(|| {
                if release.prerelease {
                    "Prerelease published without release notes.".to_string()
                } else {
                    "Release published without release notes.".to_string()
                }
            }),
            published_at: release.published_at,
            url: release.html_url,
        })
        .collect())
}

pub fn download(release: AppRelease) -> AppResult<DownloadedUpdate> {
    let release = normalize_release(release)?;
    let cache = update_version_cache_dir(&release.version)?;
    fs::create_dir_all(&cache)?;
    let archive_path = cache.join(PORTABLE_ARCHIVE_NAME);
    let temp_path = cache.join(format!("{PORTABLE_ARCHIVE_NAME}.download"));
    let mut response = reqwest::blocking::get(&release.portable.url)?.error_for_status()?;
    let mut output = fs::File::create(&temp_path)?;
    if let Err(error) = std::io::copy(&mut response, &mut output) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    drop(output);

    let actual_sha = sha256_file(&temp_path)?;
    if !actual_sha.eq_ignore_ascii_case(&release.portable.sha256) {
        let _ = fs::remove_file(&temp_path);
        return Err(AppError::Message(format!(
            "portable zip SHA256 校验失败，期望 {}，实际 {}",
            release.portable.sha256, actual_sha
        )));
    }
    if archive_path.exists() {
        fs::remove_file(&archive_path)?;
    }
    fs::rename(temp_path, &archive_path)?;

    Ok(DownloadedUpdate {
        version: release.version,
        archive_path: archive_path.display().to_string(),
        sha256: actual_sha,
        downloaded_at: Utc::now().to_rfc3339(),
    })
}

pub fn prepare_portable_install(
    app: tauri::AppHandle,
    downloaded: DownloadedUpdate,
) -> AppResult<PortableInstallPlan> {
    let exe_path = std::env::current_exe()?;
    let app_dir = exe_path
        .parent()
        .ok_or_else(|| AppError::Message("无法定位当前程序目录".to_string()))?
        .to_path_buf();
    let version = update_version_component(&downloaded.version)?;
    validate_sha256(&downloaded.sha256)?;
    let cache = update_version_cache_dir(&version)?;
    fs::create_dir_all(&cache)?;
    let archive_path = PathBuf::from(&downloaded.archive_path);
    if !archive_path.exists() {
        return msg("更新包不存在，请重新下载");
    }
    let expected_archive = cache.join(PORTABLE_ARCHIVE_NAME);
    if archive_path.canonicalize()? != expected_archive.canonicalize()? {
        return msg("更新包路径不在当前版本缓存目录，请重新下载");
    }
    let actual_sha = sha256_file(&archive_path)?;
    if !actual_sha.eq_ignore_ascii_case(&downloaded.sha256) {
        return Err(AppError::Message(format!(
            "portable zip SHA256 校验失败，期望 {}，实际 {}",
            downloaded.sha256, actual_sha
        )));
    }

    let staging_dir = cache.join("staged");
    if staging_dir.exists() {
        fs::remove_dir_all(&staging_dir)?;
    }
    fs::create_dir_all(&staging_dir)?;
    extract_zip(&archive_path, &staging_dir)?;

    let script_path = cache.join("apply-portable-update.ps1");
    write_install_script(&script_path, &staging_dir, &app_dir, &exe_path)?;

    let mut command = Command::new("powershell");
    command
        .arg("-ExecutionPolicy")
        .arg("Bypass")
        .arg("-File")
        .arg(&script_path)
        .arg("-Pid")
        .arg(std::process::id().to_string());
    hide_command_window(&mut command);
    command.spawn()?;

    app.exit(0);

    Ok(PortableInstallPlan {
        version,
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

fn update_version_cache_dir(version: &str) -> AppResult<PathBuf> {
    Ok(update_cache_dir()?.join(update_version_component(version)?))
}

fn validate_update_source_url(url: &str) -> AppResult<()> {
    let parsed = parse_https_url(url)?;
    if parsed.host_str() != Some(TRUSTED_GITHUB_HOST)
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return msg("更新源必须使用官方 GitHub Release 的 release-index.json");
    }
    let Some(segments) = parsed.path_segments() else {
        return msg("更新源路径无效");
    };
    let segments = segments.collect::<Vec<_>>();
    let is_latest = segments
        == [
            TRUSTED_GITHUB_OWNER,
            TRUSTED_GITHUB_REPO,
            "releases",
            "latest",
            "download",
            RELEASE_INDEX_ASSET,
        ];
    let is_tagged = segments.len() == 6
        && segments[0] == TRUSTED_GITHUB_OWNER
        && segments[1] == TRUSTED_GITHUB_REPO
        && segments[2] == "releases"
        && segments[3] == "download"
        && update_version_component(segments[4]).is_ok()
        && segments[5] == RELEASE_INDEX_ASSET;
    if is_latest || is_tagged {
        Ok(())
    } else {
        msg("更新源必须使用官方 GitHub Release 的 release-index.json")
    }
}

fn validate_portable_asset_url(url: &str, version: &str) -> AppResult<()> {
    let parsed = parse_https_url(url)?;
    if parsed.host_str() != Some(TRUSTED_GITHUB_HOST)
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return msg("更新包必须来自官方 GitHub Release asset");
    }
    let Some(segments) = parsed.path_segments() else {
        return msg("更新包 URL 路径无效");
    };
    let segments = segments.collect::<Vec<_>>();
    let expected_tag = format!("v{version}");
    let expected_asset = format!("KairosPatch-v{version}-portable.zip");
    if segments
        == [
            TRUSTED_GITHUB_OWNER,
            TRUSTED_GITHUB_REPO,
            "releases",
            "download",
            &expected_tag,
            &expected_asset,
        ]
    {
        Ok(())
    } else {
        msg("更新包必须来自匹配版本的官方 GitHub Release asset")
    }
}

fn parse_https_url(url: &str) -> AppResult<reqwest::Url> {
    let parsed =
        reqwest::Url::parse(url).map_err(|_| AppError::Message("URL 格式无效".to_string()))?;
    if parsed.scheme() != "https" {
        return msg("更新 URL 必须使用 https://");
    }
    Ok(parsed)
}

fn normalize_release_index(index: ReleaseIndex) -> AppResult<Vec<AppRelease>> {
    if index.app_id != UPDATE_APP_ID {
        return msg("更新索引 app_id 不匹配");
    }
    index
        .releases
        .into_iter()
        .map(normalize_release)
        .collect::<AppResult<Vec<_>>>()
}

fn normalize_release(mut release: AppRelease) -> AppResult<AppRelease> {
    let version = update_version_component(&release.version)?;
    validate_portable_asset_url(&release.portable.url, &version)?;
    validate_sha256(&release.portable.sha256)?;
    release.version = version;
    Ok(release)
}

fn update_version_component(version: &str) -> AppResult<String> {
    let version = version.trim();
    let normalized = version.strip_prefix('v').unwrap_or(version);
    if normalized.is_empty()
        || normalized.starts_with('v')
        || version.contains('/')
        || version.contains('\\')
        || version.contains(':')
        || version.contains("..")
        || version
            .chars()
            .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        return msg("更新版本号不是安全的路径组件");
    }
    Version::parse(normalized)
        .map(|version| version.to_string())
        .map_err(|_| AppError::Message("更新版本号必须是 SemVer".to_string()))
}

fn validate_sha256(value: &str) -> AppResult<()> {
    if value.len() == 64 && value.chars().all(|ch| ch.is_ascii_hexdigit()) {
        Ok(())
    } else {
        msg("更新包 SHA256 格式无效")
    }
}

fn github_repo_from_update_source(url: &str) -> Option<(String, String)> {
    validate_update_source_url(url).ok()?;
    Some((
        TRUSTED_GITHUB_OWNER.to_string(),
        TRUSTED_GITHUB_REPO.to_string(),
    ))
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
    let archive_file = fs::File::open(archive_path)?;
    let mut archive =
        ZipArchive::new(archive_file).map_err(|error| AppError::Message(error.to_string()))?;

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

fn write_install_script(
    script_path: &Path,
    staging_dir: &Path,
    app_dir: &Path,
    exe_path: &Path,
) -> AppResult<()> {
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

#[cfg(windows)]
fn hide_command_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;

    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_command_window(_command: &mut Command) {}

fn escape_ps(path: &Path) -> String {
    path.display().to_string().replace('"', "`\"")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid_release() -> AppRelease {
        AppRelease {
            version: "0.1.2".to_string(),
            notes: None,
            published_at: None,
            portable: PortableAsset {
                url: "https://github.com/SevenThRe/karios-patch/releases/download/v0.1.2/KairosPatch-v0.1.2-portable.zip".to_string(),
                sha256: "21c97f4211097137a951ece018f12753e932e1ded2c018296f68f0037c0c8db8".to_string(),
                size: Some(3545512),
            },
        }
    }

    #[test]
    fn accepts_only_official_update_index_urls() {
        assert!(validate_update_source_url(DEFAULT_UPDATE_SOURCE).is_ok());
        assert!(
            validate_update_source_url(
                "https://github.com/SevenThRe/karios-patch/releases/download/v0.1.2/release-index.json"
            )
            .is_ok()
        );
        assert!(
            validate_update_source_url(
                "https://github.com/attacker/karios-patch/releases/latest/download/release-index.json"
            )
            .is_err()
        );
        assert!(
            validate_update_source_url(
                "https://example.com/SevenThRe/karios-patch/releases/latest/download/release-index.json"
            )
            .is_err()
        );
    }

    #[test]
    fn validates_release_asset_matches_version_and_repo() {
        assert!(normalize_release(valid_release()).is_ok());

        let mut wrong_version = valid_release();
        wrong_version.portable.url = "https://github.com/SevenThRe/karios-patch/releases/download/v0.1.3/KairosPatch-v0.1.3-portable.zip".to_string();
        assert!(normalize_release(wrong_version).is_err());

        let mut wrong_repo = valid_release();
        wrong_repo.portable.url = "https://github.com/SevenThRe/other/releases/download/v0.1.2/KairosPatch-v0.1.2-portable.zip".to_string();
        assert!(normalize_release(wrong_repo).is_err());
    }

    #[test]
    fn validates_update_version_as_safe_semver_component() {
        assert_eq!(update_version_component("0.1.2").unwrap(), "0.1.2");
        assert_eq!(update_version_component("v0.1.2").unwrap(), "0.1.2");
        assert!(update_version_component("vv0.1.2").is_err());
        assert!(update_version_component("../0.1.2").is_err());
        assert!(update_version_component("0.1.2\\evil").is_err());
        assert!(update_version_component("latest").is_err());
    }

    #[test]
    fn rejects_invalid_release_index_metadata() {
        let index = ReleaseIndex {
            app_id: "other-app".to_string(),
            latest: "0.1.2".to_string(),
            releases: vec![valid_release()],
        };

        assert!(normalize_release_index(index).is_err());
    }

    #[test]
    fn rejects_invalid_sha256_metadata() {
        let mut release = valid_release();
        release.portable.sha256 = "not-a-sha".to_string();

        assert!(normalize_release(release).is_err());
    }
}
