use crate::{
    error::{AppError, AppResult},
    preferences, state,
    updater::DEFAULT_UPDATE_SOURCE,
};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::Command,
    thread,
    time::{Duration, Instant},
};
use zip::{ZipWriter, write::SimpleFileOptions};

const GITHUB_ISSUE_URL: &str =
    "https://github.com/SevenThRe/karios-patch/issues/new?template=feedback.yml";
const MAX_APP_LOG_BYTES: u64 = 512 * 1024;
const MAX_UI_LOGS: usize = 40;
const LOG_TAIL_LINES: usize = 500;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiLogEntry {
    pub level: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppLogRequest {
    pub level: String,
    pub message: String,
    pub context: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackRequest {
    pub issue_type: String,
    pub title: String,
    pub description: String,
    pub reproduction_steps: String,
    pub include_logs: bool,
    pub include_config: bool,
    pub attachment_paths: Vec<String>,
    pub contact: Option<String>,
    pub instance_dir: Option<String>,
    pub old_source: Option<String>,
    pub new_source: Option<String>,
    pub update_source: Option<String>,
    pub patch_version: Option<String>,
    pub ui_logs: Vec<UiLogEntry>,
    pub open_issue: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeedbackPackage {
    pub report_path: String,
    pub archive_path: String,
    pub issue_body_path: String,
    pub app_log_path: String,
    pub issue_url: String,
}

#[derive(Debug, Serialize)]
struct DiagnosticReport {
    feedback: FeedbackForm,
    automatic: AutomaticMetadata,
    selected_paths: SelectedPaths,
    included_files: IncludedFiles,
    recent_ui_logs: Vec<UiLogEntry>,
    generated_at: String,
}

#[derive(Debug, Serialize)]
struct FeedbackForm {
    issue_type: String,
    title: String,
    description: String,
    reproduction_steps: String,
    include_logs: bool,
    include_config: bool,
    attachments: Vec<String>,
    contact: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AutomaticMetadata {
    tool_name: String,
    tool_version: String,
    patch_version: String,
    os: String,
    arch: String,
    java_version: String,
    error_hash: String,
    log_tail: String,
    config_snapshot: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct SelectedPaths {
    instance_dir: Option<String>,
    old_source: Option<String>,
    new_source: Option<String>,
    update_source: String,
    preferences_path: String,
    app_log_path: String,
}

#[derive(Debug, Serialize)]
struct IncludedFiles {
    diagnostic_archive: String,
    report: String,
    issue_body: String,
    app_log: Option<String>,
    attachments: Vec<String>,
}

#[derive(Debug, Serialize)]
struct InstanceStateSummary {
    state_path: String,
    installed_version: String,
    managed_files: usize,
    user_overrides: usize,
    backups: usize,
}

#[derive(Debug, Serialize)]
struct AppLogRecord {
    timestamp: String,
    level: String,
    message: String,
    context: Option<String>,
}

pub fn append_app_log(request: AppLogRequest) -> AppResult<PathBuf> {
    let record = AppLogRecord {
        timestamp: Utc::now().to_rfc3339(),
        level: normalize_level(&request.level),
        message: request.message,
        context: request.context,
    };
    let path = app_log_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    rotate_app_log_if_needed(&path)?;
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    writeln!(file, "{}", serde_json::to_string(&record)?)?;
    Ok(path)
}

pub fn create_feedback_package(
    request: FeedbackRequest,
    app_version: &str,
) -> AppResult<FeedbackPackage> {
    let request = normalize_feedback_request(request)?;
    let timestamp = Utc::now();
    let diagnostics_dir = diagnostics_dir()?;
    fs::create_dir_all(&diagnostics_dir)?;
    let file_stem = format!("kairos-feedback-{}", timestamp.format("%Y%m%dT%H%M%SZ"));
    let report_path = diagnostics_dir.join(format!("{file_stem}.json"));
    let archive_path = diagnostics_dir.join(format!("{file_stem}.zip"));
    let issue_body_path = diagnostics_dir.join(format!("{file_stem}-github-issue.md"));
    let app_log_path = app_log_path()?;

    let update_source = request
        .update_source
        .clone()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_UPDATE_SOURCE.to_string());
    let log_tail = if request.include_logs {
        read_log_tail(&app_log_path, LOG_TAIL_LINES)?
    } else {
        "(not included)".to_string()
    };
    let config_snapshot = if request.include_config {
        build_config_snapshot(&request, &app_log_path, &update_source)?
    } else {
        serde_json::Value::String("(not included)".to_string())
    };
    let patch_version = request
        .patch_version
        .clone()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            summarize_instance_state(request.instance_dir.as_deref())
                .ok()
                .flatten()
                .map(|summary| summary.installed_version)
        })
        .unwrap_or_else(|| "unknown".to_string());
    let automatic = AutomaticMetadata {
        tool_name: "Kairos Patch".to_string(),
        tool_version: app_version.to_string(),
        patch_version,
        os: windows_os_label(),
        arch: normalized_arch(),
        java_version: java_version_label(),
        error_hash: error_hash(&request, &log_tail),
        log_tail,
        config_snapshot,
    };

    let selected_paths = SelectedPaths {
        instance_dir: request.instance_dir.as_deref().map(redact_path),
        old_source: request.old_source.as_deref().map(redact_path),
        new_source: request.new_source.as_deref().map(redact_path),
        update_source,
        preferences_path: redact_path(&preferences::preferences_path()?.display().to_string()),
        app_log_path: redact_path(&app_log_path.display().to_string()),
    };
    let included_files = IncludedFiles {
        diagnostic_archive: archive_path.display().to_string(),
        report: report_path.display().to_string(),
        issue_body: issue_body_path.display().to_string(),
        app_log: request.include_logs.then(|| "app.log.jsonl".to_string()),
        attachments: request
            .attachment_paths
            .iter()
            .map(|path| redact_path(path))
            .collect(),
    };
    let report = DiagnosticReport {
        feedback: FeedbackForm {
            issue_type: request.issue_type.clone(),
            title: request.title.clone(),
            description: request.description.clone(),
            reproduction_steps: request.reproduction_steps.clone(),
            include_logs: request.include_logs,
            include_config: request.include_config,
            attachments: request
                .attachment_paths
                .iter()
                .map(|path| redact_path(path))
                .collect(),
            contact: request.contact.clone(),
        },
        automatic,
        selected_paths,
        included_files,
        recent_ui_logs: request
            .ui_logs
            .iter()
            .rev()
            .take(MAX_UI_LOGS)
            .cloned()
            .collect(),
        generated_at: timestamp.to_rfc3339(),
    };

    fs::write(&report_path, serde_json::to_vec_pretty(&report)?)?;
    fs::write(&issue_body_path, build_issue_body(&report, &archive_path)?)?;
    write_feedback_archive(
        &archive_path,
        &report_path,
        &issue_body_path,
        &app_log_path,
        &request,
    )?;

    let issue_url = build_issue_url(&request.title);
    if request.open_issue {
        tauri_plugin_opener::open_url(&issue_url, None::<String>)
            .map_err(|error| AppError::Message(error.to_string()))?;
    }

    Ok(FeedbackPackage {
        report_path: report_path.display().to_string(),
        archive_path: archive_path.display().to_string(),
        issue_body_path: issue_body_path.display().to_string(),
        app_log_path: app_log_path.display().to_string(),
        issue_url,
    })
}

fn normalize_feedback_request(mut request: FeedbackRequest) -> AppResult<FeedbackRequest> {
    request.issue_type = match request.issue_type.trim() {
        "Bug" | "建议" | "Suggestion" | "安装失败" | "Install failure" | "更新失败"
        | "Update failure" | "其他" | "Other" => request.issue_type.trim().to_string(),
        _ => "其他".to_string(),
    };
    request.title = request.title.trim().to_string();
    request.description = request.description.trim().to_string();
    request.reproduction_steps = request.reproduction_steps.trim().to_string();
    request.contact = request
        .contact
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());
    request.attachment_paths = request
        .attachment_paths
        .into_iter()
        .filter(|value| !value.trim().is_empty())
        .collect();

    if request.title.is_empty() {
        return Err(AppError::Message("Feedback title is required".to_string()));
    }
    if request.description.is_empty() {
        return Err(AppError::Message(
            "Feedback description is required".to_string(),
        ));
    }
    Ok(request)
}

fn build_config_snapshot(
    request: &FeedbackRequest,
    app_log_path: &Path,
    update_source: &str,
) -> AppResult<serde_json::Value> {
    let instance_state = summarize_instance_state(request.instance_dir.as_deref())?;
    Ok(serde_json::json!({
        "instanceDir": request.instance_dir.as_deref().map(redact_path),
        "oldSource": request.old_source.as_deref().map(redact_path),
        "newSource": request.new_source.as_deref().map(redact_path),
        "updateSource": update_source,
        "preferencesPath": redact_path(&preferences::preferences_path()?.display().to_string()),
        "appLogPath": redact_path(&app_log_path.display().to_string()),
        "instanceState": instance_state.map(|state| serde_json::json!({
            "statePath": redact_path(&state.state_path),
            "installedVersion": state.installed_version,
            "managedFiles": state.managed_files,
            "userOverrides": state.user_overrides,
            "backups": state.backups,
        })),
    }))
}

fn write_feedback_archive(
    archive_path: &Path,
    report_path: &Path,
    issue_body_path: &Path,
    app_log_path: &Path,
    request: &FeedbackRequest,
) -> AppResult<()> {
    let archive = fs::File::create(archive_path)?;
    let mut zip = ZipWriter::new(archive);
    let options = SimpleFileOptions::default();

    add_file_to_zip(&mut zip, report_path, "report.json", options)?;
    add_file_to_zip(&mut zip, issue_body_path, "github-issue-body.md", options)?;
    if request.include_logs && app_log_path.exists() {
        add_file_to_zip(&mut zip, app_log_path, "app.log.jsonl", options)?;
    }

    if request.include_config
        && let Some(instance_dir) = request.instance_dir.as_deref()
    {
        let root = Path::new(instance_dir).join(".packdelta");
        add_optional_file_to_zip(
            &mut zip,
            &root.join("state.json"),
            "packdelta/state.json",
            options,
        )?;
        add_recent_conflict_notes(&mut zip, &root.join("conflicts"), options)?;
    }

    add_selected_attachments(&mut zip, &request.attachment_paths, options)?;
    zip.finish()
        .map_err(|error| AppError::Message(error.to_string()))?;
    Ok(())
}

fn add_selected_attachments<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    attachment_paths: &[String],
    options: SimpleFileOptions,
) -> AppResult<()> {
    let mut used_names = HashSet::new();
    for path in attachment_paths {
        let source = Path::new(path);
        if !source.is_file() {
            continue;
        }
        let Some(file_name) = source.file_name().and_then(|value| value.to_str()) else {
            continue;
        };
        let zip_name = unique_attachment_name(file_name, &mut used_names);
        add_file_to_zip(zip, source, &format!("attachments/{zip_name}"), options)?;
    }
    Ok(())
}

fn unique_attachment_name(file_name: &str, used_names: &mut HashSet<String>) -> String {
    if used_names.insert(file_name.to_string()) {
        return file_name.to_string();
    }
    let path = Path::new(file_name);
    let stem = path
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("attachment");
    let extension = path.extension().and_then(|value| value.to_str());
    for index in 2.. {
        let candidate = match extension {
            Some(extension) => format!("{stem}-{index}.{extension}"),
            None => format!("{stem}-{index}"),
        };
        if used_names.insert(candidate.clone()) {
            return candidate;
        }
    }
    unreachable!("attachment name loop should always return")
}

fn add_recent_conflict_notes<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    conflicts_dir: &Path,
    options: SimpleFileOptions,
) -> AppResult<()> {
    if !conflicts_dir.is_dir() {
        return Ok(());
    }
    let mut readmes = Vec::new();
    collect_conflict_readmes(conflicts_dir, &mut readmes)?;
    readmes.sort_by_key(|path| {
        fs::metadata(path)
            .and_then(|metadata| metadata.modified())
            .ok()
    });
    readmes.reverse();
    for path in readmes.into_iter().take(5) {
        let relative = path
            .strip_prefix(conflicts_dir)
            .map_err(|_| AppError::UnsafePath(path.display().to_string()))?
            .to_string_lossy()
            .replace('\\', "/");
        add_file_to_zip(
            zip,
            &path,
            &format!("packdelta/conflicts/{relative}"),
            options,
        )?;
    }
    Ok(())
}

fn collect_conflict_readmes(root: &Path, readmes: &mut Vec<PathBuf>) -> AppResult<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_conflict_readmes(&path, readmes)?;
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("README.txt"))
        {
            readmes.push(path);
        }
    }
    Ok(())
}

fn add_optional_file_to_zip<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    source: &Path,
    name: &str,
    options: SimpleFileOptions,
) -> AppResult<()> {
    if source.exists() {
        add_file_to_zip(zip, source, name, options)?;
    }
    Ok(())
}

fn add_file_to_zip<W: Write + std::io::Seek>(
    zip: &mut ZipWriter<W>,
    source: &Path,
    name: &str,
    options: SimpleFileOptions,
) -> AppResult<()> {
    zip.start_file(name, options)
        .map_err(|error| AppError::Message(error.to_string()))?;
    let bytes = fs::read(source)?;
    zip.write_all(&bytes)?;
    Ok(())
}

fn summarize_instance_state(instance_dir: Option<&str>) -> AppResult<Option<InstanceStateSummary>> {
    let Some(instance_dir) = instance_dir.filter(|value| !value.trim().is_empty()) else {
        return Ok(None);
    };
    let instance_path = Path::new(instance_dir);
    let Some(current_state) = state::read_state(instance_path)? else {
        return Ok(None);
    };
    Ok(Some(InstanceStateSummary {
        state_path: state::state_path(instance_path).display().to_string(),
        installed_version: current_state.installed_version,
        managed_files: current_state.managed_files.len(),
        user_overrides: current_state.user_overrides.len(),
        backups: current_state.backups.len(),
    }))
}

fn build_issue_body(report: &DiagnosticReport, archive_path: &Path) -> AppResult<String> {
    let metadata = serde_json::to_string_pretty(&report.automatic)?;
    Ok(format!(
        "## Issue Type\n{}\n\n## Title\n{}\n\n## Description\n{}\n\n## Reproduction Steps\n{}\n\n## Upload Consent\n- Upload logs: {}\n- Upload config: {}\n\n## Screenshots / Attachments\n{}\n\n## Contact\n{}\n\n## Automatic Metadata\n```json\n{}\n```\n\n## Diagnostic Package\nGenerated locally:\n\n```text\n{}\n```\n\nAttach this zip only if you are comfortable sharing the selected data.\n",
        report.feedback.issue_type,
        report.feedback.title,
        report.feedback.description,
        report.feedback.reproduction_steps,
        yes_no(report.feedback.include_logs),
        yes_no(report.feedback.include_config),
        if report.feedback.attachments.is_empty() {
            "(none selected)".to_string()
        } else {
            report.feedback.attachments.join("\n")
        },
        report
            .feedback
            .contact
            .as_deref()
            .unwrap_or("(not provided)"),
        metadata,
        archive_path.display()
    ))
}

fn build_issue_url(title: &str) -> String {
    format!("{GITHUB_ISSUE_URL}&title={}", percent_encode(title))
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

fn app_data_dir() -> AppResult<PathBuf> {
    dirs::data_local_dir()
        .ok_or_else(|| {
            AppError::Message("Unable to locate local application data directory".to_string())
        })
        .map(|base| base.join("KairosPatch"))
}

fn diagnostics_dir() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("diagnostics"))
}

fn app_log_path() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("logs").join("app.log.jsonl"))
}

fn rotate_app_log_if_needed(path: &Path) -> AppResult<()> {
    if !path.exists() || fs::metadata(path)?.len() <= MAX_APP_LOG_BYTES {
        return Ok(());
    }
    let rotated = path.with_extension("log.jsonl.1");
    if rotated.exists() {
        fs::remove_file(&rotated)?;
    }
    fs::rename(path, rotated)?;
    Ok(())
}

fn read_log_tail(path: &Path, max_lines: usize) -> AppResult<String> {
    if !path.exists() {
        return Ok("(no app log yet)".to_string());
    }
    let content = fs::read_to_string(path)?;
    let lines: Vec<&str> = content.lines().collect();
    let start = lines.len().saturating_sub(max_lines);
    Ok(lines[start..].join("\n"))
}

fn normalize_level(value: &str) -> String {
    match value {
        "info" | "warn" | "error" => value.to_string(),
        _ => "info".to_string(),
    }
}

fn normalized_arch() -> String {
    match std::env::consts::ARCH {
        "x86_64" => "x64".to_string(),
        value => value.to_string(),
    }
}

fn windows_os_label() -> String {
    let output = run_hidden_command("cmd", &["/C", "ver"]);
    output
        .filter(|value| value.to_ascii_lowercase().contains("windows"))
        .unwrap_or_else(|| {
            if std::env::consts::OS == "windows" {
                "Windows".to_string()
            } else {
                std::env::consts::OS.to_string()
            }
        })
}

fn java_version_label() -> String {
    run_hidden_command("java", &["-version"])
        .and_then(|value| value.lines().next().map(|line| line.trim().to_string()))
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "not found".to_string())
}

fn run_hidden_command(program: &str, args: &[&str]) -> Option<String> {
    let mut command = Command::new(program);
    command.args(args);
    hide_command_window(&mut command);
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if child.try_wait().ok().flatten().is_some() {
            let output = child.wait_with_output().ok()?;
            let combined = format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            let normalized = combined.trim().replace("\r\n", "\n");
            return (!normalized.is_empty()).then_some(normalized);
        }
        thread::sleep(Duration::from_millis(25));
    }
    let _ = child.kill();
    let output = child.wait_with_output().ok()?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let normalized = combined.trim().replace("\r\n", "\n");
    (!normalized.is_empty()).then_some(normalized)
}

#[cfg(windows)]
fn hide_command_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x08000000;
    command.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn hide_command_window(_command: &mut Command) {}

fn error_hash(request: &FeedbackRequest, log_tail: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(request.issue_type.as_bytes());
    hasher.update(request.title.as_bytes());
    hasher.update(request.description.as_bytes());
    hasher.update(request.reproduction_steps.as_bytes());
    hasher.update(log_tail.as_bytes());
    let digest = hasher.finalize();
    digest[..8]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn redact_path(value: &str) -> String {
    let mut redacted = value.to_string();
    if let Some(home) = dirs::home_dir().and_then(|path| path.to_str().map(ToOwned::to_owned)) {
        redacted = redacted.replace(&home, "%USERPROFILE%");
    }
    redacted
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_encode_escapes_spaces_and_symbols() {
        assert_eq!(percent_encode("a b+c"), "a%20b%2Bc");
    }

    #[test]
    fn issue_url_points_to_feedback_template() {
        let url = build_issue_url("Bug: failed update");

        assert!(url.contains("template=feedback.yml"));
        assert!(url.contains("Bug%3A%20failed%20update"));
    }

    #[test]
    fn normalizes_feedback_request_requires_title_and_description() {
        let request = FeedbackRequest {
            issue_type: "unknown".to_string(),
            title: " Crash ".to_string(),
            description: " Failed ".to_string(),
            reproduction_steps: " Step ".to_string(),
            include_logs: true,
            include_config: false,
            attachment_paths: vec!["".to_string(), "C:/shot.png".to_string()],
            contact: Some(" ".to_string()),
            instance_dir: None,
            old_source: None,
            new_source: None,
            update_source: None,
            patch_version: None,
            ui_logs: Vec::new(),
            open_issue: false,
        };

        let normalized = normalize_feedback_request(request).expect("request should be valid");

        assert_eq!(normalized.issue_type, "其他");
        assert_eq!(normalized.title, "Crash");
        assert_eq!(normalized.description, "Failed");
        assert_eq!(normalized.attachment_paths, vec!["C:/shot.png"]);
        assert!(normalized.contact.is_none());
    }
}
