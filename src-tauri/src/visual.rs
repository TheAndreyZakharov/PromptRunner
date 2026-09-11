use crate::error::{AppError, AppResult};
use crate::model::Zone;
use crate::model::{Profile, ScreenCapture, ScreenInfo, TemplateDefaults};
use base64::Engine;
use image::DynamicImage;
use screenshots::Screen;
use std::fs;
use std::io::Cursor;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenerationState {
    Busy,
    SendReady,
    Empty,
    Unknown,
}

pub fn default_template_paths(theme: &str) -> TemplateDefaults {
    let variant = if theme.eq_ignore_ascii_case("light") {
        "light"
    } else {
        "dark"
    };
    let candidates = [
        std::env::current_dir().ok().map(|path| path.join("assets")),
        Some(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets")),
        std::env::current_exe().ok().and_then(|path| {
            path.ancestors()
                .map(|ancestor| ancestor.join("assets"))
                .find(|candidate| candidate.is_dir())
        }),
    ];
    let asset = |name: &str| {
        candidates
            .iter()
            .flatten()
            .map(|directory| directory.join(format!("{name}_{variant}.png")))
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
    };
    TemplateDefaults {
        sample_busy: asset("writing"),
        sample_ready: asset("send"),
        sample_empty: asset("no_prompt"),
    }
}

pub fn apply_default_templates(profile: &mut Profile, theme: &str) -> bool {
    let defaults = default_template_paths(theme);
    let Some(zone) = profile
        .zones
        .iter_mut()
        .find(|zone| zone.name == "generation_state")
    else {
        return false;
    };
    let mut changed = false;
    if zone.sample_busy.is_none() && defaults.sample_busy.is_some() {
        zone.sample_busy = defaults.sample_busy;
        changed = true;
    }
    if zone.sample_ready.is_none() && defaults.sample_ready.is_some() {
        zone.sample_ready = defaults.sample_ready;
        changed = true;
    }
    if zone.sample_empty.is_none() && defaults.sample_empty.is_some() {
        zone.sample_empty = defaults.sample_empty;
        changed = true;
    }
    changed
}

pub fn screens() -> AppResult<Vec<ScreenInfo>> {
    Screen::all()
        .map_err(|e| AppError::message(format!("Не удалось получить список экранов: {e}")))
        .map(|screens| {
            screens
                .into_iter()
                .enumerate()
                .map(|(index, screen)| {
                    let info = screen.display_info;
                    ScreenInfo {
                        index: index as u32,
                        id: info.id,
                        x: info.x,
                        y: info.y,
                        width: info.width,
                        height: info.height,
                        scale_factor: info.scale_factor,
                        is_primary: info.is_primary,
                    }
                })
                .collect()
        })
}

pub fn screen_capture(index: u32) -> AppResult<ScreenCapture> {
    let screens = Screen::all()
        .map_err(|e| AppError::message(format!("Не удалось получить список экранов: {e}")))?;
    let screen = screens
        .get(index as usize)
        .ok_or_else(|| AppError::message("Выбранный экран больше недоступен"))?;
    let info = screen.display_info;
    let image = screen
        .capture()
        .map_err(|e| AppError::message(format!("Не удалось снять экран: {e}")))?;
    let image_width = image.width();
    let image_height = image.height();
    let mut bytes = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(image)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .map_err(|e| AppError::message(format!("Не удалось подготовить снимок экрана: {e}")))?;
    let data_url = format!(
        "data:image/png;base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
    );
    Ok(ScreenCapture {
        screen: ScreenInfo {
            index,
            id: info.id,
            x: info.x,
            y: info.y,
            width: info.width,
            height: info.height,
            scale_factor: info.scale_factor,
            is_primary: info.is_primary,
        },
        data_url,
        image_width,
        image_height,
    })
}

pub fn save_template(zone: &Zone, path: &str) -> AppResult<()> {
    if !zone.is_valid() {
        return Err(AppError::message("Сначала задайте observation-зону"));
    }
    let screens = Screen::all()
        .map_err(|e| AppError::message(format!("Не удалось получить список экранов: {e}")))?;
    let screen = screens
        .get(zone.screen_index as usize)
        .ok_or_else(|| AppError::message("Экран профиля не найден"))?;
    let destination = Path::new(path);
    if let Some(parent) = destination.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    let image = screen
        .capture_area(zone.x, zone.y, zone.width, zone.height)
        .map_err(|e| AppError::message(format!("Не удалось снять шаблон: {e}")))?;
    image
        .save(destination)
        .map_err(|e| AppError::message(format!("Не удалось сохранить шаблон: {e}")))?;
    let _ = fs::metadata(path)?;
    Ok(())
}

pub fn sample(zone: &Zone) -> AppResult<DynamicImage> {
    let screens =
        Screen::all().map_err(|e| AppError::message(format!("Не удалось получить экраны: {e}")))?;
    let screen = screens
        .get(zone.screen_index as usize)
        .ok_or_else(|| AppError::message("Экран профиля не найден"))?;
    let image = screen
        .capture_area(zone.x, zone.y, zone.width, zone.height)
        .map_err(|e| AppError::message(format!("Не удалось снять observation-зону: {e}")))?;
    Ok(DynamicImage::ImageRgba8(image))
}

pub fn similarity(actual: &DynamicImage, template_path: &Path) -> AppResult<f32> {
    let template = image::open(template_path).map_err(|e| {
        AppError::message(format!("Не найден шаблон {}: {e}", template_path.display()))
    })?;
    let target = actual
        .resize_exact(
            template.width(),
            template.height(),
            image::imageops::FilterType::Triangle,
        )
        .to_rgba8();
    let reference = template.to_rgba8();
    let mut total = 0.0_f32;
    for (a, b) in target.pixels().zip(reference.pixels()) {
        total +=
            a.0.iter()
                .zip(b.0.iter())
                .map(|(left, right)| (*left as f32 - *right as f32).abs())
                .sum::<f32>();
    }
    let pixels = (target.width() * target.height() * 4) as f32;
    Ok(1.0 - (total / pixels / 255.0))
}

pub fn detect(zone: &Zone, threshold: f32) -> AppResult<GenerationState> {
    let actual = sample(zone)?;
    let busy = zone
        .sample_busy
        .as_deref()
        .map(|path| similarity(&actual, Path::new(path)))
        .transpose()?
        .unwrap_or(0.0);
    let ready = zone
        .sample_ready
        .as_deref()
        .map(|path| similarity(&actual, Path::new(path)))
        .transpose()?
        .unwrap_or(0.0);
    let empty = zone
        .sample_empty
        .as_deref()
        .map(|path| similarity(&actual, Path::new(path)))
        .transpose()?
        .unwrap_or(0.0);
    if busy >= threshold && busy >= ready && busy >= empty {
        Ok(GenerationState::Busy)
    } else if empty >= threshold && empty >= busy && empty >= ready {
        Ok(GenerationState::Empty)
    } else if ready >= threshold {
        Ok(GenerationState::SendReady)
    } else {
        Ok(GenerationState::Unknown)
    }
}

pub fn detect_empty(zone: &Zone, threshold: f32) -> AppResult<bool> {
    let path = zone
        .sample_empty
        .as_deref()
        .ok_or_else(|| AppError::message("Не указан empty-шаблон observation-зоны"))?;
    Ok(similarity(&sample(zone)?, Path::new(path))? >= threshold)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_repository_visual_templates() {
        let templates = default_template_paths("dark");
        assert!(templates
            .sample_busy
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file()));
        assert!(templates
            .sample_ready
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file()));
        assert!(templates
            .sample_empty
            .as_deref()
            .is_some_and(|path| Path::new(path).is_file()));
    }
}
