use crate::{
    backup,
    diagnostics::{self, AppLogRequest, FeedbackPackage, FeedbackRequest},
    diff::{self, ManifestDiff},
    error::{AppError, AppResult},
    hash::{sha256_bytes, sha256_file},
    instance,
    manifest::{
        FileType, ManifestFile, ManifestScanProgress, Owner, PackManifest, read_source_file,
        resolve_safe, scan_pack_source as scan_source,
        scan_pack_source_with_progress as scan_source_with_progress, source_file_exists,
        write_manifest,
    },
    patch::{self, ApplyResult, RollbackResult, UpdatePlan},
    preferences::{self, AppPreferences},
    state,
    updater::{
        self, AppRelease, AppUpdateCheck, ChangelogRelease, DownloadedUpdate, PortableInstallPlan,
        UpdateSourceConfig,
    },
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{Cursor, Read},
    path::{Path, PathBuf},
};
use tauri::{AppHandle, Emitter};
use zip::ZipArchive;

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceDiffPreview {
    pub path: String,
    pub old_text: String,
    pub new_text: String,
    pub language: String,
    pub notice: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConservativeUpdatePlan {
    pub mode: String,
    pub target_version: String,
    pub auto_actions: Vec<ConservativeAction>,
    pub review_items: Vec<ReviewItem>,
    pub protected_items: Vec<ProtectedItem>,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConservativeAction {
    pub path: String,
    pub target_path: Option<String>,
    pub action: String,
    pub reason: String,
    pub mod_name: Option<String>,
    pub from_version: Option<String>,
    pub to_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewItem {
    pub id: String,
    pub path: String,
    pub kind: String,
    pub reason: String,
    pub default_choice: String,
    pub choices: Vec<String>,
    pub mod_name: Option<String>,
    pub mod_id: Option<String>,
    pub local_version: Option<String>,
    pub target_version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtectedItem {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum ConservativeReviewChoices {
    Map(BTreeMap<String, String>),
    List(Vec<ConservativeReviewChoice>),
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConservativeReviewChoice {
    pub id: Option<String>,
    pub path: Option<String>,
    pub choice: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConservativeApplyResult {
    pub backup_id: Option<String>,
    pub target_version: String,
    pub applied_changes: Vec<ConservativeAppliedChange>,
    pub preserved_paths: Vec<String>,
    pub protected_paths: Vec<String>,
    pub state_path: String,
    pub logs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConservativeAppliedChange {
    pub path: String,
    pub action: String,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OperationProgress {
    pub operation_id: String,
    pub stage: String,
    pub message: String,
    pub current: usize,
    pub total: usize,
    pub percent: u8,
    pub path: Option<String>,
    pub done: bool,
}

#[derive(Debug, Clone)]
struct ModInfo {
    id: String,
    name: Option<String>,
    version: Option<String>,
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
    let old_manifest = scan_source(
        Path::new(&old_source),
        None,
        None,
        Some("old-local".to_string()),
    )?;
    let new_manifest = scan_source(
        Path::new(&new_source),
        None,
        None,
        Some("new-local".to_string()),
    )?;
    let diff = diff::compare(&old_manifest, &new_manifest);
    Ok(CompareResult {
        old_manifest,
        new_manifest,
        diff,
    })
}

#[tauri::command]
pub fn read_source_diff(
    old_source: String,
    new_source: String,
    path: String,
) -> AppResult<SourceDiffPreview> {
    const MAX_PREVIEW_BYTES: usize = 512 * 1024;

    let old_bytes = read_source_file(Path::new(&old_source), &path).ok();
    let new_bytes = read_source_file(Path::new(&new_source), &path).ok();
    let language = language_from_path(&path);

    let old_text = preview_text(old_bytes.as_deref(), MAX_PREVIEW_BYTES);
    let new_text = preview_text(new_bytes.as_deref(), MAX_PREVIEW_BYTES);
    let notice = if old_text.is_none() || new_text.is_none() {
        Some("This file is binary, too large, missing from one side, or not valid UTF-8. Text diff preview is limited to readable files.".to_string())
    } else {
        None
    };

    Ok(SourceDiffPreview {
        path,
        old_text: old_text.unwrap_or_default(),
        new_text: new_text.unwrap_or_default(),
        language,
        notice,
    })
}

#[tauri::command]
pub fn preview_conservative_update(
    instance_dir: String,
    target_source: String,
) -> AppResult<ConservativeUpdatePlan> {
    preview_conservative_update_with_progress(instance_dir, target_source, None)
}

fn preview_conservative_update_with_progress(
    instance_dir: String,
    target_source: String,
    mut on_progress: Option<&mut dyn FnMut(&str, ManifestScanProgress)>,
) -> AppResult<ConservativeUpdatePlan> {
    let mut emit_instance_scan = |progress: ManifestScanProgress| {
        if let Some(callback) = on_progress.as_deref_mut() {
            callback("Scanning current instance", progress);
        }
    };
    let instance = scan_source_with_progress(
        Path::new(&instance_dir),
        None,
        None,
        Some("current-instance".to_string()),
        Some(&mut emit_instance_scan),
    )?;
    let mut emit_target_scan = |progress: ManifestScanProgress| {
        if let Some(callback) = on_progress.as_deref_mut() {
            callback("Scanning target source", progress);
        }
    };
    let target = scan_source_with_progress(
        Path::new(&target_source),
        None,
        None,
        Some("target-local".to_string()),
        Some(&mut emit_target_scan),
    )?;
    let instance_by_path = by_path(&instance.files);
    let target_by_path = by_path(&target.files);
    let instance_mods = mod_index(Path::new(&instance_dir), &instance.files);
    let target_mods = mod_index(Path::new(&target_source), &target.files);
    let mut auto_actions = Vec::new();
    let mut review_items = Vec::new();
    let mut protected_items = Vec::new();
    let mut logs = vec![
        "No historical baseline was provided. Conservative mode will not delete unknown local files automatically.".to_string(),
    ];

    for target_file in &target.files {
        let Some(local_file) = instance_by_path.get(&target_file.path) else {
            if is_user_asset(target_file) {
                protected_items.push(ProtectedItem {
                    path: target_file.path.clone(),
                    reason: "目标包中的用户资产路径不会自动写入实例".to_string(),
                });
            } else {
                auto_actions.push(ConservativeAction {
                    path: target_file.path.clone(),
                    target_path: Some(target_file.path.clone()),
                    action: "add_missing_target_file".to_string(),
                    reason: "目标包有此文件，实例中不存在".to_string(),
                    mod_name: mod_label(target_mods.get(&target_file.path)),
                    from_version: None,
                    to_version: target_mods
                        .get(&target_file.path)
                        .and_then(|info| info.version.clone()),
                });
            }
            continue;
        };

        if local_file.sha256 == target_file.sha256 {
            continue;
        }

        if target_file.file_type == FileType::Mod {
            let local_mod = instance_mods.get(&local_file.path);
            let target_mod = target_mods.get(&target_file.path);
            if same_mod(local_mod, target_mod) {
                auto_actions.push(ConservativeAction {
                    path: target_file.path.clone(),
                    target_path: Some(target_file.path.clone()),
                    action: "replace_same_mod".to_string(),
                    reason: "检测到相同 mod_id 的目标版本".to_string(),
                    mod_name: mod_label(target_mod).or_else(|| mod_label(local_mod)),
                    from_version: local_mod.and_then(|info| info.version.clone()),
                    to_version: target_mod.and_then(|info| info.version.clone()),
                });
            } else {
                review_items.push(review_item(
                    &target_file.path,
                    "same_path_unknown_mod_changed",
                    "同路径模组内容不同，但无法确认是同一模组升级",
                    "keep",
                    vec!["keep", "replace_with_target"],
                    local_mod,
                    target_mod,
                ));
            }
        } else if is_reviewable_config(target_file) {
            review_items.push(review_item(
                &target_file.path,
                "same_path_config_changed",
                "当前实例配置与目标包不同；无历史基线时不能自动覆盖",
                "keep",
                vec!["keep", "save_target_as_new", "use_target"],
                None,
                None,
            ));
        } else if is_user_asset(target_file) {
            protected_items.push(ProtectedItem {
                path: target_file.path.clone(),
                reason: "用户资产路径默认保护".to_string(),
            });
        } else {
            review_items.push(review_item(
                &target_file.path,
                "same_path_file_changed",
                "同路径文件内容不同，需要确认是否采用目标包版本",
                "keep",
                vec!["keep", "replace_with_target"],
                None,
                None,
            ));
        }
    }

    for local_file in &instance.files {
        if target_by_path.contains_key(&local_file.path) {
            continue;
        }

        if local_file.file_type == FileType::Mod {
            let local_mod = instance_mods.get(&local_file.path);
            if let Some((target_path, target_mod)) = find_same_mod_target(local_mod, &target_mods) {
                auto_actions.push(ConservativeAction {
                    path: local_file.path.clone(),
                    target_path: Some(target_path.clone()),
                    action: "replace_local_mod_with_target_mod".to_string(),
                    reason: format!("目标包中存在相同 mod_id 的文件 {}", target_path),
                    mod_name: mod_label(Some(target_mod)).or_else(|| mod_label(local_mod)),
                    from_version: local_mod.and_then(|info| info.version.clone()),
                    to_version: target_mod.version.clone(),
                });
            } else {
                review_items.push(review_item(
                    &local_file.path,
                    "local_only_mod",
                    "目标包未包含此本地模组；可能是用户添加，也可能是旧整合包已移除",
                    "keep",
                    vec!["keep", "remove"],
                    local_mod,
                    None,
                ));
            }
        } else if is_user_asset(local_file) {
            protected_items.push(ProtectedItem {
                path: local_file.path.clone(),
                reason: "用户资产路径默认保护".to_string(),
            });
        } else {
            review_items.push(review_item(
                &local_file.path,
                "local_only_file",
                "目标包未包含此本地文件；无历史基线时需要确认",
                "keep",
                vec!["keep", "remove"],
                None,
                None,
            ));
        }
    }

    logs.push(format!(
        "Conservative preview: {} automatic actions, {} review items, {} protected items.",
        auto_actions.len(),
        review_items.len(),
        protected_items.len()
    ));

    Ok(ConservativeUpdatePlan {
        mode: "conservative".to_string(),
        target_version: target.version,
        auto_actions,
        review_items,
        protected_items,
        logs,
    })
}

#[tauri::command]
pub fn preview_update(
    instance_dir: String,
    old_source: String,
    new_source: String,
) -> AppResult<UpdatePlan> {
    let old_manifest = scan_source(
        Path::new(&old_source),
        None,
        None,
        Some("old-local".to_string()),
    )?;
    let new_manifest = scan_source(
        Path::new(&new_source),
        None,
        None,
        Some("new-local".to_string()),
    )?;
    let diff = diff::compare(&old_manifest, &new_manifest);
    patch::build_plan(
        Path::new(&instance_dir),
        Path::new(&old_source),
        Path::new(&new_source),
        &old_manifest,
        &new_manifest,
        &diff,
    )
}

#[tauri::command]
pub fn apply_update(
    instance_dir: String,
    old_source: String,
    new_source: String,
) -> AppResult<ApplyResult> {
    let old_manifest = scan_source(
        Path::new(&old_source),
        None,
        None,
        Some("old-local".to_string()),
    )?;
    let new_manifest = scan_source(
        Path::new(&new_source),
        None,
        None,
        Some("new-local".to_string()),
    )?;
    patch::apply_update(
        Path::new(&instance_dir),
        Path::new(&old_source),
        Path::new(&new_source),
        old_manifest,
        new_manifest,
    )
}

#[tauri::command]
pub fn apply_update_tracked(
    app: AppHandle,
    operation_id: String,
    instance_dir: String,
    old_source: String,
    new_source: String,
) -> AppResult<ApplyResult> {
    emit_operation_progress(
        &app,
        &operation_id,
        "scanning",
        "Scanning official sources",
        0,
        1,
        None,
        false,
    );
    let mut emit_old_scan = |progress: ManifestScanProgress| {
        emit_manifest_progress(&app, &operation_id, "Scanning baseline source", progress);
    };
    let old_manifest = scan_source_with_progress(
        Path::new(&old_source),
        None,
        None,
        Some("old-local".to_string()),
        Some(&mut emit_old_scan),
    )?;
    let mut emit_new_scan = |progress: ManifestScanProgress| {
        emit_manifest_progress(&app, &operation_id, "Scanning target source", progress);
    };
    let new_manifest = scan_source_with_progress(
        Path::new(&new_source),
        None,
        None,
        Some("new-local".to_string()),
        Some(&mut emit_new_scan),
    )?;
    let mut emit_patch_progress = |progress: patch::PatchProgress| {
        emit_operation_progress(
            &app,
            &operation_id,
            progress.stage,
            &progress.message,
            progress.current,
            progress.total,
            progress.path.as_deref(),
            false,
        );
    };
    let result = patch::apply_update_with_progress(
        Path::new(&instance_dir),
        Path::new(&old_source),
        Path::new(&new_source),
        old_manifest,
        new_manifest,
        Some(&mut emit_patch_progress),
    )?;
    emit_operation_progress(
        &app,
        &operation_id,
        "complete",
        "Update completed",
        1,
        1,
        None,
        true,
    );
    Ok(result)
}

#[tauri::command]
pub fn apply_conservative_update(
    instance_dir: String,
    target_source: String,
    review_choices: Option<ConservativeReviewChoices>,
) -> AppResult<ConservativeApplyResult> {
    apply_conservative_update_inner(instance_dir, target_source, review_choices, None)
}

#[tauri::command]
pub fn apply_conservative_update_tracked(
    app: AppHandle,
    operation_id: String,
    instance_dir: String,
    target_source: String,
    review_choices: Option<ConservativeReviewChoices>,
) -> AppResult<ConservativeApplyResult> {
    let mut emit_progress =
        |stage: &str, message: String, current: usize, total: usize, path: Option<String>| {
            emit_operation_progress(
                &app,
                &operation_id,
                stage,
                &message,
                current,
                total,
                path.as_deref(),
                false,
            );
        };
    emit_operation_progress(
        &app,
        &operation_id,
        "scanning",
        "Scanning current instance and target source",
        0,
        1,
        None,
        false,
    );
    let result = apply_conservative_update_inner(
        instance_dir,
        target_source,
        review_choices,
        Some(&mut emit_progress),
    )?;
    emit_operation_progress(
        &app,
        &operation_id,
        "complete",
        "Update completed",
        1,
        1,
        None,
        true,
    );
    Ok(result)
}

fn apply_conservative_update_inner(
    instance_dir: String,
    target_source: String,
    review_choices: Option<ConservativeReviewChoices>,
    mut on_progress: Option<&mut dyn FnMut(&str, String, usize, usize, Option<String>)>,
) -> AppResult<ConservativeApplyResult> {
    let instance_dir = Path::new(&instance_dir);
    let target_source = Path::new(&target_source);
    validate_conservative_instance(instance_dir)?;

    let mut emit_scan = |label: &str, progress: ManifestScanProgress| {
        report_conservative_progress(
            &mut on_progress,
            "scanning",
            format!(
                "{}: {}",
                label,
                progress.path.as_deref().unwrap_or(&progress.source)
            ),
            progress.current,
            progress.total.max(1),
            progress.path,
        );
    };
    let plan = preview_conservative_update_with_progress(
        instance_dir.display().to_string(),
        target_source.display().to_string(),
        Some(&mut emit_scan),
    )?;
    let mut emit_target_rescan = |progress: ManifestScanProgress| {
        report_conservative_progress(
            &mut on_progress,
            "scanning",
            format!(
                "Scanning target source: {}",
                progress.path.as_deref().unwrap_or(&progress.source)
            ),
            progress.current,
            progress.total.max(1),
            progress.path,
        );
    };
    let target = scan_source_with_progress(
        target_source,
        None,
        None,
        Some("target-local".to_string()),
        Some(&mut emit_target_rescan),
    )?;
    let target_by_path = by_path(&target.files);
    let choices = normalize_review_choices(review_choices)?;
    validate_conservative_choices(&choices, &plan)?;
    let mut touched_paths = BTreeSet::new();
    let mut backup_files = Vec::new();
    let backup_id = backup::make_backup_id("conservative", &target.version);
    let state_before = match state::read_state(instance_dir)? {
        Some(existing) => existing,
        None => {
            let instance_manifest = scan_source(
                instance_dir,
                None,
                None,
                Some("current-instance".to_string()),
            )?;
            let instance_digest = manifest_digest(&instance_manifest)?;
            state::build_state(&instance_manifest, instance_digest)
        }
    };
    let mut applied_changes = Vec::new();
    let mut preserved_paths = Vec::new();
    let protected_paths = plan
        .protected_items
        .iter()
        .map(|item| item.path.clone())
        .collect::<Vec<_>>();
    let mut logs = vec![
        "Applying no-baseline conservative update.".to_string(),
        "Unselected review items default to keep.".to_string(),
    ];
    let total_units = plan.auto_actions.len() + plan.review_items.len() + 2;
    let mut completed_units = 0usize;
    report_conservative_progress(
        &mut on_progress,
        "planning",
        "Prepared conservative update plan".to_string(),
        completed_units,
        total_units,
        None,
    );

    for action in &plan.auto_actions {
        apply_conservative_auto_action(
            instance_dir,
            target_source,
            &target_by_path,
            action,
            &backup_id,
            &mut backup_files,
            &mut touched_paths,
            &mut applied_changes,
        )?;
        completed_units += 1;
        report_conservative_progress(
            &mut on_progress,
            "writing",
            format!("Applied automatic action: {}", action.path),
            completed_units,
            total_units,
            Some(action.path.clone()),
        );
    }

    for item in &plan.review_items {
        let choice = choices
            .get(&item.id)
            .or_else(|| choices.get(&item.path))
            .map(String::as_str)
            .unwrap_or(&item.default_choice);
        if !item.choices.iter().any(|allowed| allowed == choice) {
            return Err(AppError::Message(format!(
                "Unsupported conservative choice '{}' for {}. Allowed choices: {}",
                choice,
                item.id,
                item.choices.join(", ")
            )));
        }

        match choice {
            "keep" => {
                preserved_paths.push(item.path.clone());
                logs.push(format!("Kept local file: {}", item.path));
            }
            "remove" => {
                ensure_review_kind(item, &["local_only_mod", "local_only_file"], choice)?;
                backup_existing_once(
                    instance_dir,
                    &backup_id,
                    &item.path,
                    &mut backup_files,
                    &mut touched_paths,
                )?;
                let target = resolve_safe(instance_dir, &item.path)?;
                if target.exists() {
                    fs::remove_file(target)?;
                }
                applied_changes.push(ConservativeAppliedChange {
                    path: item.path.clone(),
                    action: "remove".to_string(),
                    source_path: None,
                });
            }
            "replace_with_target" | "use_target" => {
                ensure_review_kind(
                    item,
                    &[
                        "same_path_unknown_mod_changed",
                        "same_path_file_changed",
                        "same_path_config_changed",
                    ],
                    choice,
                )?;
                let target_file = target_by_path.get(&item.path).ok_or_else(|| {
                    AppError::Message(format!(
                        "Target source does not contain review path: {}",
                        item.path
                    ))
                })?;
                if is_user_asset(target_file) {
                    return Err(AppError::Message(format!(
                        "Refusing to overwrite protected user asset: {}",
                        item.path
                    )));
                }
                backup_existing_once(
                    instance_dir,
                    &backup_id,
                    &item.path,
                    &mut backup_files,
                    &mut touched_paths,
                )?;
                copy_source_to_instance(
                    target_source,
                    instance_dir,
                    &item.path,
                    Some(&target_file.sha256),
                )?;
                applied_changes.push(ConservativeAppliedChange {
                    path: item.path.clone(),
                    action: choice.to_string(),
                    source_path: Some(item.path.clone()),
                });
            }
            "save_target_as_new" => {
                ensure_review_kind(item, &["same_path_config_changed"], choice)?;
                let target_file = target_by_path.get(&item.path).ok_or_else(|| {
                    AppError::Message(format!(
                        "Target source does not contain review path: {}",
                        item.path
                    ))
                })?;
                let saved_path = save_target_candidate(
                    target_source,
                    instance_dir,
                    &target.version,
                    &item.path,
                    &target_file.sha256,
                )?;
                applied_changes.push(ConservativeAppliedChange {
                    path: saved_path,
                    action: "save_target_as_new".to_string(),
                    source_path: Some(item.path.clone()),
                });
            }
            other => {
                return Err(AppError::Message(format!(
                    "Unsupported conservative choice '{}' for {}",
                    other, item.id
                )));
            }
        }
        completed_units += 1;
        report_conservative_progress(
            &mut on_progress,
            "review",
            format!("Handled review item: {}", item.path),
            completed_units,
            total_units,
            Some(item.path.clone()),
        );
    }

    if !applied_changes.is_empty() {
        backup::create_backup(
            instance_dir,
            &backup_id,
            "conservative",
            &target.version,
            backup_files,
            &state_before,
        )?;
        logs.push(format!("Created backup: {}", backup_id));
    }

    let manifest_sha = manifest_digest(&target)?;
    let mut current_state = state::build_state(&target, manifest_sha);
    current_state.backups = state::read_state(instance_dir)?
        .map(|state| state.backups)
        .unwrap_or_default();
    if !applied_changes.is_empty() {
        current_state.backups.push(state::backup_record(
            backup_id.clone(),
            "conservative".to_string(),
            target.version.clone(),
        ));
    }
    sync_conservative_state_to_actual_files(instance_dir, &target, &mut current_state)?;
    current_state.user_overrides = preserved_paths.clone();
    write_manifest(
        &instance_dir
            .join(".packdelta")
            .join("manifests")
            .join(format!("{}.json", target.version)),
        &target,
    )?;
    state::write_state(instance_dir, &current_state)?;
    completed_units = total_units;
    report_conservative_progress(
        &mut on_progress,
        "state",
        "Wrote update state".to_string(),
        completed_units,
        total_units,
        None,
    );
    logs.push(format!(
        "Conservative apply completed: {} changes, {} preserved, {} protected.",
        applied_changes.len(),
        preserved_paths.len(),
        protected_paths.len()
    ));

    Ok(ConservativeApplyResult {
        backup_id: (!applied_changes.is_empty()).then_some(backup_id),
        target_version: target.version,
        applied_changes,
        preserved_paths,
        protected_paths,
        state_path: state::state_path(instance_dir).display().to_string(),
        logs,
    })
}

fn report_conservative_progress(
    on_progress: &mut Option<&mut dyn FnMut(&str, String, usize, usize, Option<String>)>,
    stage: &str,
    message: String,
    current: usize,
    total: usize,
    path: Option<String>,
) {
    if let Some(callback) = on_progress.as_deref_mut() {
        callback(stage, message, current, total, path);
    }
}

fn emit_operation_progress(
    app: &AppHandle,
    operation_id: &str,
    stage: &str,
    message: &str,
    current: usize,
    total: usize,
    path: Option<&str>,
    done: bool,
) {
    let percent = if total == 0 {
        0
    } else {
        ((current.saturating_mul(100)) / total).min(100) as u8
    };
    let _ = app.emit(
        "operation-progress",
        OperationProgress {
            operation_id: operation_id.to_string(),
            stage: stage.to_string(),
            message: message.to_string(),
            current,
            total,
            percent,
            path: path.map(str::to_string),
            done,
        },
    );
}

fn emit_manifest_progress(
    app: &AppHandle,
    operation_id: &str,
    label: &str,
    progress: ManifestScanProgress,
) {
    let path = progress.path.as_deref().unwrap_or(&progress.source);
    emit_operation_progress(
        app,
        operation_id,
        "scanning",
        &format!("{label}: {path}"),
        progress.current,
        progress.total.max(1),
        progress.path.as_deref(),
        false,
    );
}

fn preview_text(bytes: Option<&[u8]>, max_preview_bytes: usize) -> Option<String> {
    let bytes = bytes?;
    if bytes.len() > max_preview_bytes || bytes.contains(&0) {
        return None;
    }
    String::from_utf8(bytes.to_vec()).ok()
}

fn language_from_path(path: &str) -> String {
    match Path::new(path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "json" => "json",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "html" => "html",
        "css" => "css",
        "md" => "markdown",
        "xml" => "xml",
        "properties" | "cfg" | "conf" | "txt" | "log" => "plaintext",
        _ => "plaintext",
    }
    .to_string()
}

fn by_path(files: &[ManifestFile]) -> BTreeMap<String, &ManifestFile> {
    files.iter().map(|file| (file.path.clone(), file)).collect()
}

fn is_user_asset(file: &ManifestFile) -> bool {
    let path = file.path.to_ascii_lowercase();
    matches!(
        path.as_str(),
        "options.txt" | "optionsof.txt" | "servers.dat"
    ) || path.starts_with("saves/")
        || path.starts_with("shaderpacks/")
        || path.starts_with("resourcepacks/")
        || path.starts_with("journeymap/")
        || path.starts_with("xaero/")
        || path.starts_with("screenshots/")
        || path.starts_with("logs/")
        || path.starts_with("crash-reports/")
}

fn is_reviewable_config(file: &ManifestFile) -> bool {
    matches!(file.file_type, FileType::Config | FileType::Script)
}

fn same_mod(left: Option<&ModInfo>, right: Option<&ModInfo>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.id == right.id,
        _ => false,
    }
}

fn find_same_mod_target<'a>(
    local_mod: Option<&ModInfo>,
    target_mods: &'a BTreeMap<String, ModInfo>,
) -> Option<(&'a String, &'a ModInfo)> {
    let local_mod = local_mod?;
    target_mods
        .iter()
        .find(|(_, target_mod)| target_mod.id == local_mod.id)
}

fn mod_label(info: Option<&ModInfo>) -> Option<String> {
    info.map(|info| info.name.clone().unwrap_or_else(|| info.id.clone()))
}

fn review_item(
    path: &str,
    kind: &str,
    reason: &str,
    default_choice: &str,
    choices: Vec<&str>,
    local_mod: Option<&ModInfo>,
    target_mod: Option<&ModInfo>,
) -> ReviewItem {
    ReviewItem {
        id: format!("{}:{}", kind, path),
        path: path.to_string(),
        kind: kind.to_string(),
        reason: reason.to_string(),
        default_choice: default_choice.to_string(),
        choices: choices.into_iter().map(str::to_string).collect(),
        mod_name: mod_label(target_mod).or_else(|| mod_label(local_mod)),
        mod_id: target_mod.or(local_mod).map(|info| info.id.clone()),
        local_version: local_mod.and_then(|info| info.version.clone()),
        target_version: target_mod.and_then(|info| info.version.clone()),
    }
}

fn mod_index(source: &Path, files: &[ManifestFile]) -> BTreeMap<String, ModInfo> {
    let mut mods = BTreeMap::new();
    for file in files.iter().filter(|file| file.file_type == FileType::Mod) {
        let info = read_source_file(source, &file.path)
            .ok()
            .and_then(|bytes| parse_mod_info(&bytes))
            .unwrap_or_else(|| fallback_mod_info(&file.path));
        mods.insert(file.path.clone(), info);
    }
    mods
}

fn parse_mod_info(bytes: &[u8]) -> Option<ModInfo> {
    let mut archive = ZipArchive::new(Cursor::new(bytes)).ok()?;
    let candidates = [
        "fabric.mod.json",
        "quilt.mod.json",
        "META-INF/mods.toml",
        "mcmod.info",
    ];
    for candidate in candidates {
        let Ok(mut entry) = archive.by_name(candidate) else {
            continue;
        };
        let mut content = String::new();
        if entry.read_to_string(&mut content).is_err() {
            continue;
        }
        if candidate.ends_with(".json") {
            if let Some(info) = parse_json_mod_info(&content) {
                return Some(info);
            }
        } else if candidate.ends_with(".toml") {
            if let Some(info) = parse_toml_mod_info(&content) {
                return Some(info);
            }
        } else if let Some(info) = parse_mcmod_info(&content) {
            return Some(info);
        }
    }
    None
}

fn parse_json_mod_info(content: &str) -> Option<ModInfo> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let id = value.get("id")?.as_str()?.to_string();
    let name = value
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let version = value
        .get("version")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    Some(ModInfo { id, name, version })
}

fn parse_mcmod_info(content: &str) -> Option<ModInfo> {
    let value: serde_json::Value = serde_json::from_str(content).ok()?;
    let item = value.as_array()?.first()?;
    let id = item.get("modid")?.as_str()?.to_string();
    let name = item
        .get("name")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    let version = item
        .get("version")
        .and_then(|value| value.as_str())
        .map(str::to_string);
    Some(ModInfo { id, name, version })
}

fn parse_toml_mod_info(content: &str) -> Option<ModInfo> {
    let id = find_quoted_value(content, "modId")?;
    let name = find_quoted_value(content, "displayName");
    let version = find_quoted_value(content, "version");
    Some(ModInfo { id, name, version })
}

fn find_quoted_value(content: &str, key: &str) -> Option<String> {
    content.lines().find_map(|line| {
        let line = line.trim();
        let (left, right) = line.split_once('=')?;
        if left.trim() != key {
            return None;
        }
        Some(
            right
                .trim()
                .trim_matches('"')
                .trim_matches('\'')
                .to_string(),
        )
    })
}

fn fallback_mod_info(path: &str) -> ModInfo {
    let file_name = Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path);
    let id = file_name
        .split('-')
        .next()
        .unwrap_or(file_name)
        .to_ascii_lowercase();
    ModInfo {
        id,
        name: Some(file_name.to_string()),
        version: None,
    }
}

fn normalize_review_choices(
    review_choices: Option<ConservativeReviewChoices>,
) -> AppResult<BTreeMap<String, String>> {
    let mut choices = BTreeMap::new();
    match review_choices {
        None => {}
        Some(ConservativeReviewChoices::Map(map)) => {
            choices = map;
        }
        Some(ConservativeReviewChoices::List(list)) => {
            for item in list {
                let key = item.id.or(item.path).ok_or_else(|| {
                    AppError::Message(
                        "Conservative review choice requires either id or path".to_string(),
                    )
                })?;
                choices.insert(key, item.choice);
            }
        }
    }
    Ok(choices)
}

fn validate_conservative_choices(
    choices: &BTreeMap<String, String>,
    plan: &ConservativeUpdatePlan,
) -> AppResult<()> {
    let mut known_keys = BTreeSet::new();
    for item in &plan.review_items {
        known_keys.insert(item.id.clone());
        known_keys.insert(item.path.clone());
        let choice = choices
            .get(&item.id)
            .or_else(|| choices.get(&item.path))
            .map(String::as_str)
            .unwrap_or(&item.default_choice);
        if !item.choices.iter().any(|allowed| allowed == choice) {
            return Err(AppError::Message(format!(
                "Unsupported conservative choice '{}' for {}. Allowed choices: {}",
                choice,
                item.id,
                item.choices.join(", ")
            )));
        }
    }
    if let Some(unknown) = choices.keys().find(|key| !known_keys.contains(*key)) {
        return Err(AppError::Message(format!(
            "Unknown conservative review choice key: {}",
            unknown
        )));
    }
    Ok(())
}

fn apply_conservative_auto_action(
    instance_dir: &Path,
    target_source: &Path,
    target_by_path: &BTreeMap<String, &ManifestFile>,
    action: &ConservativeAction,
    backup_id: &str,
    backup_files: &mut Vec<backup::BackupFile>,
    touched_paths: &mut BTreeSet<String>,
    applied_changes: &mut Vec<ConservativeAppliedChange>,
) -> AppResult<()> {
    match action.action.as_str() {
        "add_missing_target_file" => {
            let source_path = action.target_path.as_deref().unwrap_or(&action.path);
            let target_file = target_by_path.get(source_path).ok_or_else(|| {
                AppError::Message(format!(
                    "Target source does not contain automatic action path: {source_path}"
                ))
            })?;
            if is_user_asset(target_file) {
                return Err(AppError::Message(format!(
                    "Refusing to auto-add protected user asset: {source_path}"
                )));
            }
            if resolve_safe(instance_dir, source_path)?.exists() {
                return Err(AppError::Message(format!(
                    "Automatic add became ambiguous because the file now exists: {source_path}"
                )));
            }
            copy_source_to_instance(
                target_source,
                instance_dir,
                source_path,
                Some(&target_file.sha256),
            )?;
            applied_changes.push(ConservativeAppliedChange {
                path: source_path.to_string(),
                action: action.action.clone(),
                source_path: Some(source_path.to_string()),
            });
        }
        "replace_same_mod" => {
            let source_path = action.target_path.as_deref().unwrap_or(&action.path);
            let target_file = target_by_path.get(source_path).ok_or_else(|| {
                AppError::Message(format!(
                    "Target source does not contain automatic action path: {source_path}"
                ))
            })?;
            backup_existing_once(
                instance_dir,
                backup_id,
                &action.path,
                backup_files,
                touched_paths,
            )?;
            copy_source_to_instance(
                target_source,
                instance_dir,
                &action.path,
                Some(&target_file.sha256),
            )?;
            applied_changes.push(ConservativeAppliedChange {
                path: action.path.clone(),
                action: action.action.clone(),
                source_path: Some(source_path.to_string()),
            });
        }
        "replace_local_mod_with_target_mod" => {
            let source_path = action.target_path.as_deref().ok_or_else(|| {
                AppError::Message(format!(
                    "Automatic same-mod replacement is missing target path for {}",
                    action.path
                ))
            })?;
            let target_file = target_by_path.get(source_path).ok_or_else(|| {
                AppError::Message(format!(
                    "Target source does not contain automatic action path: {source_path}"
                ))
            })?;
            backup_existing_once(
                instance_dir,
                backup_id,
                &action.path,
                backup_files,
                touched_paths,
            )?;
            if source_path != action.path {
                backup_existing_once(
                    instance_dir,
                    backup_id,
                    source_path,
                    backup_files,
                    touched_paths,
                )?;
                let local_target = resolve_safe(instance_dir, &action.path)?;
                if local_target.exists() {
                    fs::remove_file(local_target)?;
                }
            }
            copy_source_to_instance(
                target_source,
                instance_dir,
                source_path,
                Some(&target_file.sha256),
            )?;
            applied_changes.push(ConservativeAppliedChange {
                path: action.path.clone(),
                action: action.action.clone(),
                source_path: Some(source_path.to_string()),
            });
        }
        other => {
            return Err(AppError::Message(format!(
                "Unsupported conservative automatic action '{}' for {}",
                other, action.path
            )));
        }
    }
    Ok(())
}

fn ensure_review_kind(item: &ReviewItem, supported_kinds: &[&str], choice: &str) -> AppResult<()> {
    if supported_kinds.iter().any(|kind| *kind == item.kind) {
        return Ok(());
    }
    Err(AppError::Message(format!(
        "Choice '{}' is not supported for review item kind '{}' at {}",
        choice, item.kind, item.path
    )))
}

fn copy_source_to_instance(
    source_root: &Path,
    instance_dir: &Path,
    relative_path: &str,
    expected_sha: Option<&str>,
) -> AppResult<()> {
    if !source_file_exists(source_root, relative_path)? {
        return Err(AppError::Message(format!(
            "Source file does not exist: {} -> {}",
            source_root.display(),
            relative_path
        )));
    }
    let content = read_source_file(source_root, relative_path)?;
    let actual_sha = sha256_bytes(&content);
    if let Some(expected) = expected_sha {
        if actual_sha != expected {
            return Err(AppError::Message(format!(
                "SHA256 check failed: {relative_path}"
            )));
        }
    }
    let target = resolve_safe(instance_dir, relative_path)?;
    ensure_parent(&target)?;
    fs::write(target, content)?;
    Ok(())
}

fn save_target_candidate(
    target_source: &Path,
    instance_dir: &Path,
    target_version: &str,
    relative_path: &str,
    expected_sha: &str,
) -> AppResult<String> {
    let content = read_source_file(target_source, relative_path)?;
    if sha256_bytes(&content) != expected_sha {
        return Err(AppError::Message(format!(
            "SHA256 check failed: {relative_path}"
        )));
    }
    let saved_relative = format!(
        ".packdelta/conservative-candidates/{}/{}.target",
        sanitize_path_component(target_version),
        relative_path
    );
    let destination = resolve_safe(instance_dir, &saved_relative)?;
    ensure_parent(&destination)?;
    fs::write(destination, content)?;
    Ok(saved_relative)
}

fn backup_existing_once(
    instance_dir: &Path,
    backup_id: &str,
    relative_path: &str,
    backup_files: &mut Vec<backup::BackupFile>,
    touched_paths: &mut BTreeSet<String>,
) -> AppResult<()> {
    if !touched_paths.insert(relative_path.to_string()) {
        return Ok(());
    }
    if let Some(file) = backup::copy_into_backup(instance_dir, backup_id, relative_path)? {
        backup_files.push(file);
    }
    Ok(())
}

fn sync_conservative_state_to_actual_files(
    instance_dir: &Path,
    target: &PackManifest,
    current_state: &mut state::InstanceState,
) -> AppResult<()> {
    let target_files = by_path(&target.files);
    current_state.managed_files.retain(|path, managed| {
        let Some(target_file) = target_files.get(path) else {
            return false;
        };
        if target_file.owner != Owner::Pack {
            return false;
        }
        let Ok(actual_path) = resolve_safe(instance_dir, path) else {
            return false;
        };
        let Ok(actual_sha) = sha256_file(&actual_path) else {
            return false;
        };
        actual_sha == managed.sha256
    });
    Ok(())
}

fn validate_conservative_instance(instance_dir: &Path) -> AppResult<()> {
    instance::validate_game_directory(instance_dir)
}

fn ensure_parent(path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn manifest_digest(manifest: &PackManifest) -> AppResult<String> {
    let bytes = serde_json::to_vec(manifest)?;
    use sha2::{Digest, Sha256};
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn sanitize_path_component(value: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn conservative_apply_runs_auto_actions_and_keeps_user_assets_protected() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join("instance");
        let target = temp.path().join("target");
        write_file(&instance, "mods/a.jar", b"old-a");
        write_file(&instance, "resourcepacks/user.zip", b"user-pack");
        write_file(&target, "mods/a.jar", b"new-a");
        write_file(&target, "mods/b.jar", b"new-b");
        write_file(&target, "resourcepacks/target.zip", b"target-pack");

        let result = apply_conservative_update(
            instance.display().to_string(),
            target.display().to_string(),
            None,
        )
        .unwrap();

        assert_eq!(fs::read(instance.join("mods/a.jar")).unwrap(), b"new-a");
        assert_eq!(fs::read(instance.join("mods/b.jar")).unwrap(), b"new-b");
        assert!(!instance.join("resourcepacks/target.zip").exists());
        assert!(instance.join("resourcepacks/user.zip").exists());
        assert!(result.backup_id.is_some());
        assert!(
            result
                .protected_paths
                .contains(&"resourcepacks/target.zip".to_string())
        );
    }

    #[test]
    fn conservative_apply_honors_review_choices() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join("instance");
        let target = temp.path().join("target");
        write_file(&instance, "mods/a.jar", b"same");
        write_file(&instance, "config/app.toml", b"local=true\n");
        write_file(&instance, "mods/local-only.jar", b"remove-me");
        write_file(&target, "mods/a.jar", b"same");
        write_file(&target, "config/app.toml", b"target=true\n");

        let mut choices = BTreeMap::new();
        choices.insert("config/app.toml".to_string(), "use_target".to_string());
        choices.insert("mods/local-only.jar".to_string(), "remove".to_string());
        let result = apply_conservative_update(
            instance.display().to_string(),
            target.display().to_string(),
            Some(ConservativeReviewChoices::Map(choices)),
        )
        .unwrap();

        assert_eq!(
            fs::read(instance.join("config/app.toml")).unwrap(),
            b"target=true\n"
        );
        assert!(!instance.join("mods/local-only.jar").exists());
        assert!(
            result
                .applied_changes
                .iter()
                .any(|change| change.action == "use_target" && change.path == "config/app.toml")
        );
        assert!(
            result
                .applied_changes
                .iter()
                .any(|change| change.action == "remove" && change.path == "mods/local-only.jar")
        );
    }

    #[test]
    fn conservative_apply_rejects_unsupported_review_choice() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join("instance");
        let target = temp.path().join("target");
        write_file(&instance, "mods/a.jar", b"same");
        write_file(&instance, "config/app.toml", b"local=true\n");
        write_file(&target, "mods/a.jar", b"same");
        write_file(&target, "mods/b.jar", b"new-b");
        write_file(&target, "config/app.toml", b"target=true\n");

        let mut choices = BTreeMap::new();
        choices.insert("config/app.toml".to_string(), "remove".to_string());
        let error = apply_conservative_update(
            instance.display().to_string(),
            target.display().to_string(),
            Some(ConservativeReviewChoices::Map(choices)),
        )
        .unwrap_err();

        assert!(
            error
                .to_string()
                .contains("Unsupported conservative choice")
        );
        assert_eq!(
            fs::read(instance.join("config/app.toml")).unwrap(),
            b"local=true\n"
        );
        assert!(!instance.join("mods/b.jar").exists());
    }

    #[test]
    fn conservative_apply_can_save_target_config_as_candidate() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join("instance");
        let target = temp.path().join("target");
        write_file(&instance, "mods/a.jar", b"same");
        write_file(&instance, "config/app.toml", b"local=true\n");
        write_file(&target, "mods/a.jar", b"same");
        write_file(&target, "config/app.toml", b"target=true\n");

        let choices = vec![ConservativeReviewChoice {
            id: Some("same_path_config_changed:config/app.toml".to_string()),
            path: None,
            choice: "save_target_as_new".to_string(),
        }];
        let result = apply_conservative_update(
            instance.display().to_string(),
            target.display().to_string(),
            Some(ConservativeReviewChoices::List(choices)),
        )
        .unwrap();

        assert_eq!(
            fs::read(instance.join("config/app.toml")).unwrap(),
            b"local=true\n"
        );
        let saved = result
            .applied_changes
            .iter()
            .find(|change| change.action == "save_target_as_new")
            .unwrap();
        assert_eq!(
            fs::read(resolve_safe(&instance, &saved.path).unwrap()).unwrap(),
            b"target=true\n"
        );
    }

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        if root
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name == "instance")
            && relative.starts_with("mods/")
        {
            let marker = root.join("options.txt");
            if !marker.exists() {
                fs::create_dir_all(root).unwrap();
                fs::write(&marker, b"").unwrap();
            }
        }
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }
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
    if path.contains("://") {
        return Err(AppError::UnsafePath(path));
    }
    let path = PathBuf::from(path).canonicalize()?;
    if path.is_file() {
        tauri_plugin_opener::reveal_item_in_dir(&path)
    } else {
        tauri_plugin_opener::open_path(&path, None::<String>)
    }
    .map_err(|error| crate::error::AppError::Message(error.to_string()))?;
    Ok(())
}

#[tauri::command]
pub fn load_app_preferences() -> AppResult<AppPreferences> {
    preferences::load()
}

#[tauri::command]
pub fn save_app_preferences(preferences: AppPreferences) -> AppResult<AppPreferences> {
    preferences::save(preferences)
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
pub fn fetch_changelog(index_url: String) -> AppResult<Vec<ChangelogRelease>> {
    updater::changelog(&index_url)
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

#[tauri::command]
pub fn append_app_log(request: AppLogRequest) -> AppResult<String> {
    Ok(diagnostics::append_app_log(request)?.display().to_string())
}

#[tauri::command]
pub fn create_feedback_package(request: FeedbackRequest) -> AppResult<FeedbackPackage> {
    diagnostics::create_feedback_package(request, env!("CARGO_PKG_VERSION"))
}
