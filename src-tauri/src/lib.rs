mod bank;
mod error;
mod input;
mod model;
mod profile;
mod progress;
mod prompt;
mod runner;
mod visual;
mod writer;

use model::{
    AppSettings, BankSummary, Profile, ProgressState, PromptRequest, RunConfig, ScreenCapture,
    ScreenInfo, TemplateDefaults, Zone,
};
use runner::RunnerControl;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{Manager, State, WebviewUrl, WebviewWindowBuilder};

struct AppState {
    control: RunnerControl,
}

#[tauri::command]
fn validate_bank(root: String, language: String) -> Result<BankSummary, String> {
    bank::summary(Path::new(&root), &language).map_err(|e| e.to_string())
}

#[tauri::command]
fn build_prompt(request: PromptRequest) -> Result<String, String> {
    prompt::build(&request).map_err(|e| e.to_string())
}

#[tauri::command]
fn list_profiles() -> Result<Vec<Profile>, String> {
    profile::list().map_err(|e| e.to_string())
}

#[tauri::command]
fn save_profile(value: Profile) -> Result<(), String> {
    profile::save(&value).map_err(|e| e.to_string())
}

#[tauri::command]
fn pointer_position() -> Result<[i32; 2], String> {
    input::current_pointer()
        .map(|(x, y)| [x, y])
        .map_err(|e| e.to_string())
}

#[tauri::command]
fn test_action_zone(zone: Zone, origin_x: i32, origin_y: i32) -> Result<(), String> {
    if zone.kind != "action" {
        return Err("Тестовый клик разрешён только для action-зоны".into());
    }
    input::click_zone(&zone, 0, (origin_x, origin_y)).map_err(|e| e.to_string())
}

#[tauri::command]
fn start_run(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    config: RunConfig,
) -> Result<(), String> {
    runner::start(app, state.control.clone(), config).map_err(|e| e.to_string())
}

#[tauri::command]
fn pause_run(state: State<'_, AppState>) {
    runner::pause(&state.control);
}

#[tauri::command]
fn resume_run(state: State<'_, AppState>) {
    runner::resume(&state.control);
}

#[tauri::command]
fn toggle_pause(state: State<'_, AppState>) {
    runner::toggle_pause(&state.control);
}

#[tauri::command]
fn stop_run(state: State<'_, AppState>) {
    runner::stop(&state.control);
}

#[tauri::command]
fn save_settings(settings: AppSettings) -> Result<(), String> {
    let base = dirs::config_dir()
        .ok_or_else(|| "Не найден каталог конфигурации".to_string())?
        .join("PromptRunner");
    fs::create_dir_all(&base).map_err(|e| e.to_string())?;
    fs::write(
        base.join("settings.json"),
        serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

#[tauri::command]
fn load_settings() -> Result<AppSettings, String> {
    let path = dirs::config_dir()
        .ok_or_else(|| "Не найден каталог конфигурации".to_string())?
        .join("PromptRunner/settings.json");
    if !path.exists() {
        return Ok(AppSettings {
            allowed_languages: vec!["RU".into()],
            active_language: "RU".into(),
            poll_interval_seconds: 10,
            generation_timeout_seconds: 600,
            click_retries: 3,
            new_chat_every: 100,
            theme: "dark".into(),
            min_pause_seconds: 12,
            ready_confirmations: 2,
            preserve_clipboard: true,
            ..Default::default()
        });
    }
    let mut settings: AppSettings =
        serde_json::from_str(&fs::read_to_string(&path).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    // Older versions defaulted to the system theme. Migrate that untouched
    // default to the new dark default while keeping explicit light choices.
    if settings.theme == "system" {
        settings.theme = "dark".into();
        let _ = fs::write(
            &path,
            serde_json::to_vec_pretty(&settings).map_err(|e| e.to_string())?,
        );
    }
    Ok(settings)
}

#[tauri::command]
fn read_clipboard() -> Result<String, String> {
    writer::answer_text_from_clipboard().map_err(|e| e.to_string())
}

#[tauri::command]
fn choose_bank_directory() -> Option<String> {
    rfd::FileDialog::new()
        .set_title("Выберите корень IT-Interview-Question-Bank")
        .pick_folder()
        .map(|path| path.to_string_lossy().into_owned())
}

#[tauri::command]
fn list_screens() -> Result<Vec<ScreenInfo>, String> {
    visual::screens().map_err(|e| e.to_string())
}

#[tauri::command]
fn capture_screen(app: tauri::AppHandle, screen_index: u32) -> Result<ScreenCapture, String> {
    let main = app.get_webview_window("main");
    let calibration = app.get_webview_window("calibration");
    let main_visible = main
        .as_ref()
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    let calibration_visible = calibration
        .as_ref()
        .and_then(|window| window.is_visible().ok())
        .unwrap_or(false);
    if main_visible {
        if let Some(window) = &main {
            let _ = window.hide();
        }
    }
    if calibration_visible {
        if let Some(window) = &calibration {
            let _ = window.hide();
        }
    }
    thread::sleep(Duration::from_millis(120));
    let result = visual::screen_capture(screen_index).map_err(|e| e.to_string());
    if main_visible {
        if let Some(window) = &main {
            let _ = window.show();
        }
    }
    if calibration_visible {
        if let Some(window) = &calibration {
            let _ = window.show();
            let _ = window.set_focus();
        }
    }
    result
}

#[tauri::command]
fn save_template(zone: Zone, path: String) -> Result<(), String> {
    visual::save_template(&zone, &path).map_err(|e| e.to_string())
}

#[tauri::command]
fn default_template_paths(theme: String) -> TemplateDefaults {
    visual::default_template_paths(&theme)
}

#[tauri::command]
fn load_progress() -> Result<ProgressState, String> {
    progress::load().map_err(|e| e.to_string())
}

#[tauri::command]
fn open_calibration(app: tauri::AppHandle, screen_index: u32) -> Result<(), String> {
    visual::screens()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|item| item.index == screen_index)
        .ok_or_else(|| "Выбранный экран недоступен".to_string())?;
    let window = if let Some(window) = app.get_webview_window("calibration") {
        window
    } else {
        WebviewWindowBuilder::new(&app, "calibration", WebviewUrl::App("index.html".into()))
            .title("PromptRunner — калибровка")
            .fullscreen(false)
            .decorations(true)
            .always_on_top(false)
            .build()
            .map_err(|e| e.to_string())?
    };
    let _ = window.set_fullscreen(false);
    window
        .set_size(tauri::LogicalSize::new(1200.0, 820.0))
        .map_err(|e| e.to_string())?;
    let _ = window.center();
    window.show().map_err(|e| e.to_string())?;
    window.set_focus().map_err(|e| e.to_string())
}

pub fn run() {
    let control = RunnerControl(Arc::new(Mutex::new(runner::ControlState::default())));
    tauri::Builder::default()
        .manage(AppState { control })
        .invoke_handler(tauri::generate_handler![
            validate_bank,
            build_prompt,
            list_profiles,
            save_profile,
            pointer_position,
            test_action_zone,
            start_run,
            pause_run,
            resume_run,
            toggle_pause,
            stop_run,
            save_settings,
            load_settings,
            read_clipboard,
            choose_bank_directory,
            list_screens,
            capture_screen,
            save_template,
            default_template_paths,
            load_progress,
            open_calibration
        ])
        .run(tauri::generate_context!())
        .expect("error while running PromptRunner");
}
