use crate::error::{AppError, AppResult};
use crate::model::Profile;
use crate::visual;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;

fn profile_dir() -> AppResult<PathBuf> {
    let base =
        dirs::config_dir().ok_or_else(|| AppError::message("Не найден каталог конфигурации"))?;
    Ok(base.join("PromptRunner").join("profiles"))
}

pub fn list() -> AppResult<Vec<Profile>> {
    let dir = profile_dir()?;
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut result = Vec::new();
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.extension().and_then(|v| v.to_str()) != Some("json") {
            continue;
        }
        if let Ok(mut profile) = serde_json::from_str::<Profile>(&fs::read_to_string(&path)?) {
            if visual::apply_default_templates(&mut profile, "dark") {
                // The hydrated profile is still returned even if an existing profile
                // cannot be rewritten right now; the runner will use these paths in memory.
                let _ = save(&profile);
            }
            result.push(profile);
        }
    }
    result.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(result)
}

pub fn save(profile: &Profile) -> AppResult<()> {
    if profile.name.trim().is_empty() {
        return Err(AppError::message("Профиль должен иметь имя"));
    }
    if profile.screen_width == 0 || profile.screen_height == 0 || profile.scale_factor <= 0.0 {
        return Err(AppError::message(
            "Профиль должен быть привязан к выбранному экрану",
        ));
    }
    if !(0.5..=0.99).contains(&profile.visual_threshold) {
        return Err(AppError::message(
            "Порог visual-сравнения должен быть от 0.50 до 0.99",
        ));
    }
    if profile
        .zones
        .iter()
        .any(|zone| !zone.is_valid() || zone.screen_index != profile.screen_index)
    {
        return Err(AppError::message(
            "Все зоны профиля должны иметь положительные размеры и относиться к выбранному экрану",
        ));
    }
    let names = profile
        .zones
        .iter()
        .map(|zone| zone.name.as_str())
        .collect::<HashSet<_>>();
    for required in ["input", "send", "copy", "new_chat", "generation_state"] {
        if !names.contains(required) {
            return Err(AppError::message(format!(
                "В профиле отсутствует зона {required}"
            )));
        }
    }
    let state_zone = profile
        .zones
        .iter()
        .find(|zone| zone.name == "generation_state")
        .unwrap();
    for (name, expected_kind) in [
        ("input", "action"),
        ("send", "action"),
        ("copy", "action"),
        ("new_chat", "action"),
        ("generation_state", "observation"),
    ] {
        if profile
            .zones
            .iter()
            .find(|zone| zone.name == name)
            .map(|zone| zone.kind != expected_kind)
            .unwrap_or(true)
        {
            return Err(AppError::message(format!(
                "Зона {name} должна иметь тип {expected_kind}"
            )));
        }
    }
    for (label, path) in [
        ("busy", state_zone.sample_busy.as_deref()),
        ("send", state_zone.sample_ready.as_deref()),
        ("no_prompt", state_zone.sample_empty.as_deref()),
    ] {
        if let Some(path) = path {
            if !PathBuf::from(path).is_file() {
                return Err(AppError::message(format!(
                    "{label}-шаблон не найден: {path}"
                )));
            }
        }
    }
    let dir = profile_dir()?;
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("{}.json", profile.id));
    let temp_path = dir.join(format!(".{}.json.tmp", profile.id));
    fs::write(&temp_path, serde_json::to_vec_pretty(profile)?)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path)?;
    }
    if let Err(error) = fs::rename(&temp_path, &path) {
        let _ = fs::remove_file(&temp_path);
        return Err(error.into());
    }
    Ok(())
}
