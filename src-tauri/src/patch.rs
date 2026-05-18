use crate::{
    backup,
    diff::{ManifestDiff, RenamedFile, UpdatedFile},
    error::{AppError, AppResult, msg},
    hash::{sha256_bytes, sha256_file},
    manifest::{FileType, ManifestFile, Owner, PackManifest, Strategy, read_source_file, resolve_safe, source_file_exists, write_manifest},
    state,
};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeSet, fs, path::Path, process::Command};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatePlan {
    pub from: String,
    pub to: String,
    pub added: Vec<FileAction>,
    pub removed: Vec<FileAction>,
    pub updated: Vec<FileAction>,
    pub merged: Vec<FileAction>,
    pub renamed: Vec<RenameAction>,
    pub preserved: Vec<String>,
    pub already_current: Vec<FileAction>,
    pub conflicts: Vec<Conflict>,
    pub backup_candidates: Vec<String>,
    pub logs: Vec<OperationLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileAction {
    pub path: String,
    pub sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenameAction {
    pub from: String,
    pub to: String,
    pub sha256: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conflict {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationLog {
    pub level: LogLevel,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogLevel {
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApplyResult {
    pub backup_id: Option<String>,
    pub plan: UpdatePlan,
    pub state_path: String,
    pub logs: Vec<OperationLog>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RollbackResult {
    pub backup_id: String,
    pub restored_files: usize,
    pub state_path: String,
}

pub fn build_plan(
    instance_dir: &Path,
    old_source: &Path,
    new_source: &Path,
    old_manifest: &PackManifest,
    new_manifest: &PackManifest,
    diff: &ManifestDiff,
) -> AppResult<UpdatePlan> {
    validate_instance(instance_dir)?;
    let mut plan = UpdatePlan {
        from: old_manifest.version.clone(),
        to: new_manifest.version.clone(),
        added: Vec::new(),
        removed: Vec::new(),
        updated: Vec::new(),
        merged: Vec::new(),
        renamed: Vec::new(),
        preserved: Vec::new(),
        already_current: Vec::new(),
        conflicts: Vec::new(),
        backup_candidates: Vec::new(),
        logs: Vec::new(),
    };
    plan.log_info(format!(
        "Scanned update context: {} -> {}",
        old_manifest.version, new_manifest.version
    ));

    for file in &diff.added {
        if is_apply_managed(file) {
            let target = resolve_safe(instance_dir, &file.path)?;
            if target.exists() {
                let current_hash = sha256_file(&target)?;
                if current_hash == file.sha256 {
                    plan.mark_current(file);
                } else {
                    plan.log_warn(format!("Conflict: local file already exists for new official file {}", file.path));
                    plan.conflicts.push(Conflict {
                        path: file.path.clone(),
                        reason: "用户实例中已存在同名文件，保留用户文件".to_string(),
                    });
                }
            } else {
                plan.log_info(format!("Plan add: {}", file.path));
                plan.added.push(FileAction {
                    path: file.path.clone(),
                    sha256: Some(file.sha256.clone()),
                });
            }
        } else if is_mergeable_config(file) {
            handle_added_config(instance_dir, file, &mut plan)?;
        } else {
            plan.preserved.push(file.path.clone());
        }
    }

    for file in &diff.removed {
        handle_removed(instance_dir, file, &mut plan)?;
    }

    for UpdatedFile { old, new } in &diff.updated {
        if is_apply_managed(old) && is_apply_managed(new) {
            let target = resolve_safe(instance_dir, &old.path)?;
            if !target.exists() {
                plan.updated.push(FileAction {
                    path: new.path.clone(),
                    sha256: Some(new.sha256.clone()),
                });
                continue;
            }
            let current_hash = sha256_file(&target)?;
            if current_hash == new.sha256 {
                plan.mark_current(new);
            } else if current_hash == old.sha256 {
                plan.backup_candidates.push(old.path.clone());
                plan.log_info(format!("Plan replace: {}", new.path));
                plan.updated.push(FileAction {
                    path: new.path.clone(),
                    sha256: Some(new.sha256.clone()),
                });
            } else {
                plan.log_warn(format!("Conflict: local official file changed by user {}", old.path));
                plan.conflicts.push(Conflict {
                    path: old.path.clone(),
                    reason: "用户修改过官方文件，保留本地文件，新官方文件写入冲突目录".to_string(),
                });
            }
        } else if is_mergeable_config(old) && is_mergeable_config(new) {
            handle_updated_config(instance_dir, old_source, new_source, old, new, &mut plan)?;
        } else {
            plan.preserved.push(new.path.clone());
        }
    }

    for RenamedFile { old, new } in &diff.renamed {
        if is_apply_managed(old) && is_apply_managed(new) {
            let old_target = resolve_safe(instance_dir, &old.path)?;
            let new_target = resolve_safe(instance_dir, &new.path)?;
            if new_target.exists() && sha256_file(&new_target)? == new.sha256 && !old_target.exists() {
                plan.mark_current(new);
            } else if old_target.exists() && sha256_file(&old_target)? == old.sha256 && !new_target.exists() {
                plan.backup_candidates.push(old.path.clone());
                plan.log_info(format!("Plan rename: {} -> {}", old.path, new.path));
                plan.renamed.push(RenameAction {
                    from: old.path.clone(),
                    to: new.path.clone(),
                    sha256: new.sha256.clone(),
                });
            } else {
                plan.log_warn(format!("Conflict: rename is not safe {} -> {}", old.path, new.path));
                plan.conflicts.push(Conflict {
                    path: old.path.clone(),
                    reason: "重命名目标不安全或原文件已被用户修改".to_string(),
                });
            }
        } else {
            plan.preserved.push(new.path.clone());
        }
    }

    dedupe(&mut plan.backup_candidates);
    plan.log_info(format!(
        "Plan summary: {} executable actions, {} already current, {} conflicts",
        plan.executable_action_count(),
        plan.already_current.len(),
        plan.conflicts.len()
    ));
    Ok(plan)
}

pub fn apply_update(instance_dir: &Path, old_source: &Path, new_source: &Path, old_manifest: PackManifest, new_manifest: PackManifest) -> AppResult<ApplyResult> {
    if minecraft_running() {
        return msg("检测到 Minecraft/Java 游戏进程正在运行，请关闭游戏后再更新");
    }

    let diff = crate::diff::compare(&old_manifest, &new_manifest);
    let mut plan = build_plan(instance_dir, old_source, new_source, &old_manifest, &new_manifest, &diff)?;
    let manifest_sha = manifest_digest(&new_manifest)?;
    let mut current_state = match state::read_state(instance_dir)? {
        Some(existing) => existing,
        None => state::build_state(&old_manifest, manifest_digest(&old_manifest)?),
    };

    if !plan.has_work() {
        plan.log_info("No update actions required; instance already matches the target managed files.");
        current_state = state::build_state(&new_manifest, manifest_sha);
        current_state.backups = state::read_state(instance_dir)?
            .map(|state| state.backups)
            .unwrap_or_default();
        track_merged_configs(&mut current_state, &new_manifest, &plan);
        write_manifest(
            &instance_dir
                .join(".packdelta")
                .join("manifests")
                .join(format!("{}.json", old_manifest.version)),
            &old_manifest,
        )?;
        write_manifest(
            &instance_dir
                .join(".packdelta")
                .join("manifests")
                .join(format!("{}.json", new_manifest.version)),
            &new_manifest,
        )?;
        state::write_state(instance_dir, &current_state)?;
        let logs = plan.logs.clone();
        return Ok(ApplyResult {
            backup_id: None,
            plan,
            state_path: state::state_path(instance_dir).display().to_string(),
            logs,
        });
    }

    let backup_id = backup::make_backup_id(&old_manifest.version, &new_manifest.version);
    let mut backup_files = Vec::new();
    for path in &plan.backup_candidates {
        if let Some(file) = backup::copy_into_backup(instance_dir, &backup_id, path)? {
            backup_files.push(file);
        }
    }
    for action in plan.removed.iter().chain(plan.updated.iter()).chain(plan.merged.iter()) {
        if !plan.backup_candidates.iter().any(|p| p == &action.path) {
            if let Some(file) = backup::copy_into_backup(instance_dir, &backup_id, &action.path)? {
                backup_files.push(file);
            }
        }
    }
    backup::create_backup(instance_dir, &backup_id, &old_manifest.version, &new_manifest.version, backup_files, &current_state)?;
    plan.log_info(format!("Created backup: {}", backup_id));

    for action in plan.removed.clone() {
        let target = resolve_safe(instance_dir, &action.path)?;
        if target.exists() {
            fs::remove_file(target)?;
            plan.log_info(format!("Removed: {}", action.path));
        }
    }

    for action in plan.renamed.clone() {
        let from = resolve_safe(instance_dir, &action.from)?;
        let to = resolve_safe(instance_dir, &action.to)?;
        ensure_parent(&to)?;
        fs::rename(from, to)?;
        plan.log_info(format!("Renamed: {} -> {}", action.from, action.to));
    }

    for update in plan.updated.clone() {
        copy_from_source(new_source, instance_dir, &update.path, update.sha256.as_deref())?;
        plan.log_info(format!("Updated: {}", update.path));
    }
    for merge in plan.merged.clone() {
        merge_config_file(old_source, new_source, instance_dir, &merge.path, merge.sha256.as_deref())?;
        plan.log_info(format!("Merged config: {}", merge.path));
    }
    for add in plan.added.clone() {
        copy_from_source(new_source, instance_dir, &add.path, add.sha256.as_deref())?;
        plan.log_info(format!("Added: {}", add.path));
    }

    for conflict in plan.conflicts.clone() {
        if let Some(new_file) = new_manifest.files.iter().find(|file| file.path == conflict.path) {
            let conflict_root = instance_dir
                .join(".packdelta")
                .join("conflicts")
                .join(format!("{}_to_{}", old_manifest.version, new_manifest.version));
            copy_from_source(new_source, &conflict_root, &new_file.path, Some(&new_file.sha256))?;
            plan.log_warn(format!("Wrote conflict candidate: {}", new_file.path));
        }
    }
    write_conflict_notes(instance_dir, &old_manifest.version, &new_manifest.version, &plan)?;

    write_manifest(
        &instance_dir
            .join(".packdelta")
            .join("manifests")
            .join(format!("{}.json", old_manifest.version)),
        &old_manifest,
    )?;
    write_manifest(
        &instance_dir
            .join(".packdelta")
            .join("manifests")
            .join(format!("{}.json", new_manifest.version)),
        &new_manifest,
    )?;

    current_state = state::build_state(&new_manifest, manifest_sha);
    current_state.backups = state::read_state(instance_dir)?
        .map(|state| state.backups)
        .unwrap_or_default();
    track_merged_configs(&mut current_state, &new_manifest, &plan);
    current_state.backups.push(state::backup_record(
        backup_id.clone(),
        old_manifest.version,
        new_manifest.version,
    ));
    state::write_state(instance_dir, &current_state)?;
    plan.conflicts.sort_by(|a, b| a.path.cmp(&b.path));
    plan.log_info(format!("Wrote state: {}", state::state_path(instance_dir).display()));
    let logs = plan.logs.clone();

    Ok(ApplyResult {
        backup_id: Some(backup_id),
        plan,
        state_path: state::state_path(instance_dir).display().to_string(),
        logs,
    })
}

pub fn rollback(instance_dir: &Path, backup_id: &str) -> AppResult<RollbackResult> {
    backup::write_rollback_safety(instance_dir, backup_id)?;
    let backup_manifest = backup::read_backup_manifest(instance_dir, backup_id)?;
    let state_before = backup::read_state_before(instance_dir, backup_id)?;
    let current_state = state::read_state(instance_dir)?.unwrap_or_else(|| state_before.clone());

    for path in current_state.managed_files.keys() {
        if !state_before.managed_files.contains_key(path) {
            let target = resolve_safe(instance_dir, path)?;
            if target.exists() {
                fs::remove_file(target)?;
            }
        }
    }

    for file in &backup_manifest.files {
        let source = instance_dir
            .join(".packdelta")
            .join("backups")
            .join(backup_id)
            .join(file.backup_path.replace('/', std::path::MAIN_SEPARATOR_STR));
        let target = resolve_safe(instance_dir, &file.path)?;
        ensure_parent(&target)?;
        fs::copy(source, target)?;
    }

    state::write_state(instance_dir, &state_before)?;
    fs::write(
        instance_dir
            .join(".packdelta")
            .join("backups")
            .join(backup_id)
            .join("rollback.log"),
        format!("rollback completed for {backup_id}\n"),
    )?;

    Ok(RollbackResult {
        backup_id: backup_id.to_string(),
        restored_files: backup_manifest.files.len(),
        state_path: state::state_path(instance_dir).display().to_string(),
    })
}

fn handle_removed(instance_dir: &Path, file: &ManifestFile, plan: &mut UpdatePlan) -> AppResult<()> {
    if !is_apply_managed(file) {
        plan.preserved.push(file.path.clone());
        return Ok(());
    }
    let target = resolve_safe(instance_dir, &file.path)?;
    if !target.exists() {
        plan.mark_current(file);
        return Ok(());
    }
    let current_hash = sha256_file(&target)?;
    if current_hash == file.sha256 {
        plan.backup_candidates.push(file.path.clone());
        plan.log_info(format!("Plan remove: {}", file.path));
        plan.removed.push(FileAction {
            path: file.path.clone(),
            sha256: Some(file.sha256.clone()),
        });
    } else {
        plan.log_warn(format!("Conflict: remove target was changed locally {}", file.path));
        plan.conflicts.push(Conflict {
            path: file.path.clone(),
            reason: "删除目标已被用户修改，保留本地文件".to_string(),
        });
    }
    Ok(())
}

fn handle_added_config(instance_dir: &Path, file: &ManifestFile, plan: &mut UpdatePlan) -> AppResult<()> {
    let target = resolve_safe(instance_dir, &file.path)?;
    if target.exists() {
        let current_hash = sha256_file(&target)?;
        if current_hash == file.sha256 {
            plan.mark_current(file);
        } else {
            plan.backup_candidates.push(file.path.clone());
            plan.log_warn(format!("Conflict: new official config collides with local config {}", file.path));
            plan.conflicts.push(Conflict {
                path: file.path.clone(),
                reason: "新官方 config 与本地同名配置冲突，需要手动选择保留本地或采用官方文件".to_string(),
            });
        }
    } else {
        plan.log_info(format!("Plan add config: {}", file.path));
        plan.merged.push(FileAction {
            path: file.path.clone(),
            sha256: Some(file.sha256.clone()),
        });
    }
    Ok(())
}

fn track_merged_configs(current_state: &mut state::InstanceState, new_manifest: &PackManifest, plan: &UpdatePlan) {
    for merge in &plan.merged {
        let Some(file) = new_manifest.files.iter().find(|file| file.path == merge.path) else {
            continue;
        };
        current_state.managed_files.insert(
            file.path.clone(),
            state::ManagedFileState {
                sha256: file.sha256.clone(),
                owner: file.owner.clone(),
                version: new_manifest.version.clone(),
            },
        );
    }
}

fn write_conflict_notes(instance_dir: &Path, from: &str, to: &str, plan: &UpdatePlan) -> AppResult<()> {
    if plan.conflicts.is_empty() {
        return Ok(());
    }
    let conflict_root = instance_dir
        .join(".packdelta")
        .join("conflicts")
        .join(format!("{from}_to_{to}"));
    fs::create_dir_all(&conflict_root)?;
    let mut notes = String::from(
        "Kairos Patch conflict candidates\n\n\
         Local files were preserved. Files under this directory are the new official candidates.\n\
         Review each path and copy a candidate over the instance file only if you want to adopt it.\n\n\
         冲突处理说明：本地文件已保留。本目录下是新版官方候选文件。\n\
         请逐个比较后自行决定保留本地文件，或把候选文件复制回实例目录对应位置。\n\n",
    );
    for conflict in &plan.conflicts {
        notes.push_str(&format!("- {}: {}\n", conflict.path, conflict.reason));
    }
    fs::write(conflict_root.join("README.txt"), notes)?;
    Ok(())
}

fn handle_updated_config(
    instance_dir: &Path,
    old_source: &Path,
    new_source: &Path,
    old: &ManifestFile,
    new: &ManifestFile,
    plan: &mut UpdatePlan,
) -> AppResult<()> {
    let target = resolve_safe(instance_dir, &old.path)?;
    if !target.exists() {
        plan.merged.push(FileAction {
            path: new.path.clone(),
            sha256: Some(new.sha256.clone()),
        });
        return Ok(());
    }

    let current_hash = sha256_file(&target)?;
    if current_hash == new.sha256 {
        plan.mark_current(new);
        return Ok(());
    }
    plan.backup_candidates.push(old.path.clone());
    let can_auto_merge = can_merge_config(old_source, new_source, instance_dir, &old.path).unwrap_or(false);
    if current_hash == old.sha256 || can_auto_merge {
        plan.log_info(format!("Plan merge config: {}", new.path));
        plan.merged.push(FileAction {
            path: new.path.clone(),
            sha256: Some(new.sha256.clone()),
        });
    } else {
        plan.log_warn(format!("Conflict: config changed both locally and officially {}", old.path));
        plan.conflicts.push(Conflict {
            path: old.path.clone(),
            reason: "本地 config 与新版官方 config 修改了同一位置，需要手动决策".to_string(),
        });
    }
    Ok(())
}

impl UpdatePlan {
    fn executable_action_count(&self) -> usize {
        self.added.len()
            + self.removed.len()
            + self.updated.len()
            + self.merged.len()
            + self.renamed.len()
    }

    fn has_work(&self) -> bool {
        self.executable_action_count() > 0 || !self.conflicts.is_empty()
    }

    fn mark_current(&mut self, file: &ManifestFile) {
        self.already_current.push(FileAction {
            path: file.path.clone(),
            sha256: Some(file.sha256.clone()),
        });
        self.log_info(format!("Already current: {}", file.path));
    }

    fn log_info(&mut self, message: impl Into<String>) {
        self.logs.push(OperationLog {
            level: LogLevel::Info,
            message: message.into(),
        });
    }

    fn log_warn(&mut self, message: impl Into<String>) {
        self.logs.push(OperationLog {
            level: LogLevel::Warn,
            message: message.into(),
        });
    }
}

fn is_apply_managed(file: &ManifestFile) -> bool {
    file.owner == Owner::Pack && matches!(file.strategy, Strategy::Replace | Strategy::Delete)
}

fn is_mergeable_config(file: &ManifestFile) -> bool {
    file.file_type == FileType::Config && matches!(file.strategy, Strategy::Merge)
}

fn can_merge_config(old_source: &Path, new_source: &Path, instance_dir: &Path, relative_path: &str) -> AppResult<bool> {
    let old_text = read_utf8(old_source, relative_path)?;
    let local_text = read_utf8(instance_dir, relative_path)?;
    let new_text = read_utf8(new_source, relative_path)?;
    Ok(merge_text(&old_text, &local_text, &new_text).is_some())
}

fn merge_config_file(
    old_source: &Path,
    new_source: &Path,
    instance_dir: &Path,
    relative_path: &str,
    expected_sha: Option<&str>,
) -> AppResult<()> {
    let new_bytes = read_source_file(new_source, relative_path)?;
    if let Some(expected) = expected_sha {
        let actual_sha = sha256_bytes(&new_bytes);
        if actual_sha != expected {
            return Err(AppError::Message(format!("SHA256 校验失败: {relative_path}")));
        }
    }

    let target = resolve_safe(instance_dir, relative_path)?;
    if !target.exists() {
        copy_from_source(new_source, instance_dir, relative_path, expected_sha)?;
        return Ok(());
    }

    let old_bytes = read_source_file(old_source, relative_path)?;
    if sha256_file(&target)? == sha256_bytes(&old_bytes) {
        copy_from_source(new_source, instance_dir, relative_path, expected_sha)?;
        return Ok(());
    }

    let old_text = read_utf8(old_source, relative_path)?;
    let local_text = read_utf8(instance_dir, relative_path)?;
    let new_text = read_utf8(new_source, relative_path)?;
    let merged = merge_text(&old_text, &local_text, &new_text).ok_or_else(|| {
        AppError::Message(format!("config 自动合并失败，需要手动处理: {relative_path}"))
    })?;
    ensure_parent(&target)?;
    fs::write(target, merged)?;
    Ok(())
}

fn read_utf8(root: &Path, relative_path: &str) -> AppResult<String> {
    let bytes = read_source_file(root, relative_path)?;
    String::from_utf8(bytes)
        .map_err(|_| AppError::Message(format!("config 不是 UTF-8 文本，无法自动合并: {relative_path}")))
}

fn merge_text(old: &str, local: &str, new: &str) -> Option<String> {
    if local == new {
        return Some(local.to_string());
    }
    if local == old {
        return Some(new.to_string());
    }
    if old == new {
        return Some(local.to_string());
    }

    let old_lines = split_preserving_newlines(old);
    let local_lines = split_preserving_newlines(local);
    let new_lines = split_preserving_newlines(new);
    let shared_len = old_lines.len();

    if local_lines.len() < shared_len || new_lines.len() < shared_len {
        return None;
    }

    let mut merged = Vec::new();
    for index in 0..shared_len {
        let old_line = old_lines[index];
        let local_line = local_lines[index];
        let new_line = new_lines[index];
        if local_line == new_line {
            merged.push(local_line);
        } else if local_line == old_line {
            merged.push(new_line);
        } else if new_line == old_line {
            merged.push(local_line);
        } else {
            return None;
        }
    }

    if local_lines[shared_len..] == new_lines[shared_len..] {
        merged.extend_from_slice(&local_lines[shared_len..]);
    } else if local_lines.len() == shared_len {
        merged.extend_from_slice(&new_lines[shared_len..]);
    } else if new_lines.len() == shared_len {
        merged.extend_from_slice(&local_lines[shared_len..]);
    } else {
        return None;
    }

    Some(merged.concat())
}

fn split_preserving_newlines(value: &str) -> Vec<&str> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (index, ch) in value.char_indices() {
        if ch == '\n' {
            lines.push(&value[start..=index]);
            start = index + ch.len_utf8();
        }
    }
    if start < value.len() {
        lines.push(&value[start..]);
    }
    lines
}

fn copy_from_source(source_root: &Path, target_root: &Path, relative_path: &str, expected_sha: Option<&str>) -> AppResult<()> {
    if !source_file_exists(source_root, relative_path)? {
        return Err(AppError::Message(format!(
            "源文件不存在: {} -> {}",
            source_root.display(),
            relative_path
        )));
    }
    let content = read_source_file(source_root, relative_path)?;
    let actual_sha = sha256_bytes(&content);
    if let Some(expected) = expected_sha {
        if actual_sha != expected {
            return Err(AppError::Message(format!("SHA256 校验失败: {relative_path}")));
        }
    }
    let target = resolve_safe(target_root, relative_path)?;
    ensure_parent(&target)?;
    fs::write(target, content)?;
    Ok(())
}

fn ensure_parent(path: &Path) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    Ok(())
}

fn validate_instance(instance_dir: &Path) -> AppResult<()> {
    if !instance_dir.exists() || !instance_dir.is_dir() {
        return msg("目标实例目录不存在");
    }
    if !instance_dir.join("mods").is_dir() {
        return msg("目标实例目录缺少 mods 目录，未通过 Minecraft 整合包结构检查");
    }
    Ok(())
}

fn manifest_digest(manifest: &PackManifest) -> AppResult<String> {
    let bytes = serde_json::to_vec(manifest)?;
    use sha2::{Digest, Sha256};
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn minecraft_running() -> bool {
    if !cfg!(windows) {
        return false;
    }
    let output = Command::new("tasklist").output();
    let Ok(output) = output else {
        return false;
    };
    let text = String::from_utf8_lossy(&output.stdout).to_ascii_lowercase();
    text.contains("minecraftlauncher.exe") || text.contains("minecraft.exe")
}

fn dedupe(values: &mut Vec<String>) {
    let mut set = BTreeSet::new();
    values.retain(|value| set.insert(value.clone()));
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::scan_pack_source;
    use std::{fs, io::Write, path::Path};
    use tempfile::tempdir;
    use zip::{ZipWriter, write::SimpleFileOptions};

    #[test]
    fn apply_preserves_user_modified_pack_file_and_user_added_mod() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&old_source, "mods/remove.jar", b"remove-me");
        write_file(&new_source, "mods/a.jar", b"new-a");
        write_file(&new_source, "mods/b.jar", b"new-b");
        write_file(&instance, "mods/a.jar", b"user-edited-a");
        write_file(&instance, "mods/remove.jar", b"remove-me");
        write_file(&instance, "mods/user-extra.jar", b"user-extra");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();

        assert_eq!(fs::read(instance.join("mods/a.jar")).unwrap(), b"user-edited-a");
        assert_eq!(fs::read(instance.join("mods/b.jar")).unwrap(), b"new-b");
        assert_eq!(fs::read(instance.join("mods/user-extra.jar")).unwrap(), b"user-extra");
        assert!(!instance.join("mods/remove.jar").exists());
        assert_eq!(result.plan.conflicts.len(), 1);
        assert!(instance.join(".packdelta/state.json").exists());
    }

    #[test]
    fn rollback_restores_removed_files_without_deleting_user_extra_mod() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&old_source, "mods/remove.jar", b"remove-me");
        write_file(&new_source, "mods/a.jar", b"new-a");
        write_file(&new_source, "mods/b.jar", b"new-b");
        write_file(&instance, "mods/a.jar", b"old-a");
        write_file(&instance, "mods/remove.jar", b"remove-me");
        write_file(&instance, "mods/user-extra.jar", b"user-extra");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();
        rollback(&instance, result.backup_id.as_deref().unwrap()).unwrap();

        assert_eq!(fs::read(instance.join("mods/a.jar")).unwrap(), b"old-a");
        assert_eq!(fs::read(instance.join("mods/remove.jar")).unwrap(), b"remove-me");
        assert_eq!(fs::read(instance.join("mods/user-extra.jar")).unwrap(), b"user-extra");
    }

    #[test]
    fn apply_merges_non_overlapping_config_changes() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&new_source, "mods/a.jar", b"old-a");
        write_file(&old_source, "config/app.toml", b"enabled=true\nlimit=1\n");
        write_file(&new_source, "config/app.toml", b"enabled=true\nlimit=2\nnew-option=true\n");
        write_file(&instance, "mods/a.jar", b"old-a");
        write_file(&instance, "config/app.toml", b"enabled=false\nlimit=1\n");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();

        assert_eq!(
            fs::read_to_string(instance.join("config/app.toml")).unwrap(),
            "enabled=false\nlimit=2\nnew-option=true\n"
        );
        assert_eq!(result.plan.merged.len(), 1);
        assert!(result.plan.conflicts.is_empty());
    }

    #[test]
    fn apply_preserves_removed_config_files() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&new_source, "mods/a.jar", b"old-a");
        write_file(&old_source, "config/legacy.toml", b"legacy=true\n");
        write_file(&instance, "mods/a.jar", b"old-a");
        write_file(&instance, "config/legacy.toml", b"legacy=false\n");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();

        assert_eq!(
            fs::read_to_string(instance.join("config/legacy.toml")).unwrap(),
            "legacy=false\n"
        );
        assert!(result.plan.removed.is_empty());
        assert!(result.plan.preserved.contains(&"config/legacy.toml".to_string()));
    }

    #[test]
    fn apply_preserves_conflicting_config_for_manual_decision() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&new_source, "mods/a.jar", b"old-a");
        write_file(&old_source, "config/app.toml", b"limit=1\n");
        write_file(&new_source, "config/app.toml", b"limit=2\n");
        write_file(&instance, "mods/a.jar", b"old-a");
        write_file(&instance, "config/app.toml", b"limit=3\n");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();

        assert_eq!(fs::read_to_string(instance.join("config/app.toml")).unwrap(), "limit=3\n");
        assert_eq!(result.plan.conflicts.len(), 1);
        assert!(instance.join(".packdelta/conflicts/1.0.0_to_1.0.1/config/app.toml").exists());
        assert!(instance.join(".packdelta/conflicts/1.0.0_to_1.0.1/README.txt").exists());
    }

    #[test]
    fn rollback_removes_newly_added_merged_config() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&new_source, "mods/a.jar", b"old-a");
        write_file(&new_source, "config/new.toml", b"enabled=true\n");
        write_file(&instance, "mods/a.jar", b"old-a");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();
        assert!(instance.join("config/new.toml").exists());

        rollback(&instance, result.backup_id.as_deref().unwrap()).unwrap();

        assert!(!instance.join("config/new.toml").exists());
    }

    #[test]
    fn preview_and_apply_skip_files_that_already_match_target_manifest() {
        let temp = tempdir().unwrap();
        let old_source = temp.path().join("old");
        let new_source = temp.path().join("new");
        let instance = temp.path().join("instance");

        write_file(&old_source, "mods/a.jar", b"old-a");
        write_file(&old_source, "mods/removed.jar", b"removed");
        write_file(&old_source, "config/app.toml", b"limit=1\n");
        write_file(&new_source, "mods/a.jar", b"new-a");
        write_file(&new_source, "mods/b.jar", b"new-b");
        write_file(&new_source, "config/app.toml", b"limit=2\n");
        write_file(&instance, "mods/a.jar", b"new-a");
        write_file(&instance, "mods/b.jar", b"new-b");
        write_file(&instance, "config/app.toml", b"limit=2\n");

        let old_manifest = scan_pack_source(&old_source, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_source, None, None, Some("1.0.1".to_string())).unwrap();
        let diff = crate::diff::compare(&old_manifest, &new_manifest);
        let plan = build_plan(&instance, &old_source, &new_source, &old_manifest, &new_manifest, &diff).unwrap();

        assert_eq!(plan.executable_action_count(), 0);
        assert!(plan.conflicts.is_empty());
        assert_eq!(plan.already_current.len(), 4);

        let result = apply_update(&instance, &old_source, &new_source, old_manifest, new_manifest).unwrap();

        assert!(result.backup_id.is_none());
        assert_eq!(result.plan.executable_action_count(), 0);
        assert!(result.logs.iter().any(|log| log.message.contains("No update actions required")));
    }

    #[test]
    fn apply_update_can_read_official_sources_from_zip_files() {
        let temp = tempdir().unwrap();
        let old_zip = temp.path().join("old.zip");
        let new_zip = temp.path().join("new.zip");
        let instance = temp.path().join("instance");

        write_zip(
            &old_zip,
            &[
                ("WorldsVault/mods/a.jar", b"old-a".as_slice()),
                ("WorldsVault/mods/remove.jar", b"remove-me".as_slice()),
            ],
        );
        write_zip(
            &new_zip,
            &[
                ("WorldsVault/mods/a.jar", b"new-a".as_slice()),
                ("WorldsVault/mods/b.jar", b"new-b".as_slice()),
            ],
        );
        write_file(&instance, "mods/a.jar", b"old-a");
        write_file(&instance, "mods/remove.jar", b"remove-me");
        write_file(&instance, "mods/user-extra.jar", b"user-extra");

        let old_manifest = scan_pack_source(&old_zip, None, None, Some("1.0.0".to_string())).unwrap();
        let new_manifest = scan_pack_source(&new_zip, None, None, Some("1.0.1".to_string())).unwrap();

        assert!(old_manifest.files.iter().any(|file| file.path == "mods/a.jar"));

        let result = apply_update(&instance, &old_zip, &new_zip, old_manifest, new_manifest).unwrap();

        assert_eq!(fs::read(instance.join("mods/a.jar")).unwrap(), b"new-a");
        assert_eq!(fs::read(instance.join("mods/b.jar")).unwrap(), b"new-b");
        assert_eq!(fs::read(instance.join("mods/user-extra.jar")).unwrap(), b"user-extra");
        assert!(!instance.join("mods/remove.jar").exists());
        assert!(result.plan.conflicts.is_empty());
    }

    #[test]
    fn directory_sources_normalize_curseforge_overrides_paths() {
        let temp = tempdir().unwrap();
        let source = temp.path().join("source");

        write_file(&source, "overrides/mods/a.jar", b"a");
        write_file(&source, "overrides/config/app.toml", b"enabled=true\n");
        write_file(&source, "overrides/kubejs/server_scripts/main.js", b"ServerEvents.loaded(() => {})\n");

        let manifest = scan_pack_source(&source, None, None, Some("1.0.0".to_string())).unwrap();

        assert!(manifest.files.iter().any(|file| file.path == "mods/a.jar"));
        assert!(manifest.files.iter().any(|file| file.path == "config/app.toml"));
        assert!(manifest.files.iter().any(|file| file.path == "kubejs/server_scripts/main.js"));
        assert!(!manifest.files.iter().any(|file| file.path.starts_with("overrides/")));
    }

    fn write_file(root: &Path, relative: &str, content: &[u8]) {
        let path = root.join(relative.replace('/', std::path::MAIN_SEPARATOR_STR));
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, content).unwrap();
    }

    fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let file = fs::File::create(path).unwrap();
        let mut zip = ZipWriter::new(file);
        let options = SimpleFileOptions::default();
        for (name, content) in entries {
            zip.start_file(name, options).unwrap();
            zip.write_all(content).unwrap();
        }
        zip.finish().unwrap();
    }
}
