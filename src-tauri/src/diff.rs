use crate::manifest::{ManifestFile, PackManifest};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestDiff {
    pub from: String,
    pub to: String,
    pub added: Vec<ManifestFile>,
    pub removed: Vec<ManifestFile>,
    pub updated: Vec<UpdatedFile>,
    pub renamed: Vec<RenamedFile>,
    pub unchanged: Vec<ManifestFile>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdatedFile {
    pub old: ManifestFile,
    pub new: ManifestFile,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RenamedFile {
    pub old: ManifestFile,
    pub new: ManifestFile,
}

pub fn compare(old: &PackManifest, new: &PackManifest) -> ManifestDiff {
    let old_by_path: HashMap<_, _> = old.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let new_by_path: HashMap<_, _> = new.files.iter().map(|f| (f.path.as_str(), f)).collect();
    let mut raw_added = Vec::new();
    let mut raw_removed = Vec::new();
    let mut updated = Vec::new();
    let mut unchanged = Vec::new();

    for old_file in &old.files {
        match new_by_path.get(old_file.path.as_str()) {
            Some(new_file) if new_file.sha256 == old_file.sha256 => {
                unchanged.push((*new_file).clone())
            }
            Some(new_file) => updated.push(UpdatedFile {
                old: old_file.clone(),
                new: (*new_file).clone(),
            }),
            None => raw_removed.push(old_file.clone()),
        }
    }

    for new_file in &new.files {
        if !old_by_path.contains_key(new_file.path.as_str()) {
            raw_added.push(new_file.clone());
        }
    }

    let mut used_added = HashSet::new();
    let mut used_removed = HashSet::new();
    let mut renamed = Vec::new();

    for (old_index, old_file) in raw_removed.iter().enumerate() {
        if let Some((new_index, new_file)) =
            raw_added.iter().enumerate().find(|(index, candidate)| {
                !used_added.contains(index) && candidate.sha256 == old_file.sha256
            })
        {
            used_added.insert(new_index);
            used_removed.insert(old_index);
            renamed.push(RenamedFile {
                old: old_file.clone(),
                new: new_file.clone(),
            });
        }
    }

    let added = raw_added
        .into_iter()
        .enumerate()
        .filter_map(|(index, file)| (!used_added.contains(&index)).then_some(file))
        .collect();
    let removed = raw_removed
        .into_iter()
        .enumerate()
        .filter_map(|(index, file)| (!used_removed.contains(&index)).then_some(file))
        .collect();

    ManifestDiff {
        from: old.version.clone(),
        to: new.version.clone(),
        added,
        removed,
        updated,
        renamed,
        unchanged,
    }
}
