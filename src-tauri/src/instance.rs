use crate::error::{AppResult, msg};
use std::path::Path;

pub fn validate_game_directory(instance_dir: &Path) -> AppResult<()> {
    if !instance_dir.exists() || !instance_dir.is_dir() {
        return msg("Target instance directory does not exist");
    }
    if !instance_dir.join("mods").is_dir() {
        return msg("Target instance directory must contain a mods directory");
    }
    if !has_minecraft_marker(instance_dir) {
        return msg(
            "Target instance directory does not look like a Minecraft game directory. Select the .minecraft directory or the isolated version game directory.",
        );
    }
    Ok(())
}

fn has_minecraft_marker(instance_dir: &Path) -> bool {
    instance_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.eq_ignore_ascii_case(".minecraft"))
        || instance_dir.join("versions").is_dir()
        || instance_dir.join("assets").is_dir()
        || instance_dir.join("libraries").is_dir()
        || instance_dir.join("launcher_profiles.json").is_file()
        || instance_dir.join("options.txt").is_file()
        || instance_dir.join("servers.dat").is_file()
        || instance_dir
            .parent()
            .and_then(|parent| parent.file_name())
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.eq_ignore_ascii_case("versions"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::tempdir;

    #[test]
    fn accepts_dot_minecraft_game_directory() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join(".minecraft");
        fs::create_dir_all(instance.join("mods")).unwrap();

        validate_game_directory(&instance).unwrap();
    }

    #[test]
    fn accepts_version_isolated_game_directory() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join(".minecraft").join("versions").join("Pack");
        fs::create_dir_all(instance.join("mods")).unwrap();

        validate_game_directory(&instance).unwrap();
    }

    #[test]
    fn rejects_plain_mod_folder_without_minecraft_marker() {
        let temp = tempdir().unwrap();
        let instance = temp.path().join("not-a-game");
        fs::create_dir_all(instance.join("mods")).unwrap();

        assert!(validate_game_directory(&instance).is_err());
    }
}
