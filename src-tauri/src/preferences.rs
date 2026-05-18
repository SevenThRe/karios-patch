use crate::error::{AppError, AppResult};
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AppPreferences {
    pub instance_dir: Option<String>,
    pub old_source: Option<String>,
    pub new_source: Option<String>,
    pub locale: Option<String>,
}

pub fn preferences_path() -> AppResult<PathBuf> {
    let base = dirs::config_dir()
        .ok_or_else(|| AppError::Message("无法定位用户配置目录".to_string()))?
        .join("KairosPatch");
    Ok(base.join("preferences.json"))
}

pub fn load() -> AppResult<AppPreferences> {
    let path = preferences_path()?;
    if !path.exists() {
        return Ok(AppPreferences::default());
    }
    Ok(serde_json::from_slice(&fs::read(path)?)?)
}

pub fn save(mut preferences: AppPreferences) -> AppResult<AppPreferences> {
    preferences.instance_dir = normalize_optional_path(preferences.instance_dir);
    preferences.old_source = normalize_optional_path(preferences.old_source);
    preferences.new_source = normalize_optional_path(preferences.new_source);
    preferences.locale = preferences
        .locale
        .filter(|locale| matches!(locale.as_str(), "zh" | "en"));

    let path = preferences_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(&preferences)?)?;
    Ok(preferences)
}

fn normalize_optional_path(path: Option<String>) -> Option<String> {
    path.map(|path| path.trim().to_string())
        .filter(|path| !path.is_empty())
}
