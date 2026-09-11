use crate::error::{AppError, AppResult};
use crate::model::{ProgressState, RunEvent};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn data_dir() -> AppResult<PathBuf> {
    let base = dirs::data_local_dir()
        .or_else(dirs::config_dir)
        .ok_or_else(|| AppError::message("Не найден каталог данных приложения"))?;
    let directory = base.join("PromptRunner");
    fs::create_dir_all(&directory)?;
    Ok(directory)
}

fn now() -> String {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}

pub fn load() -> AppResult<ProgressState> {
    let path = data_dir()?.join("progress.json");
    if !path.exists() {
        return Ok(ProgressState::default());
    }
    serde_json::from_str(&fs::read_to_string(path)?).map_err(Into::into)
}

pub fn save(state: &ProgressState) -> AppResult<()> {
    let directory = data_dir()?;
    let path = directory.join("progress.json");
    let temporary = directory.join("progress.json.tmp");
    fs::write(&temporary, serde_json::to_vec_pretty(state)?)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(&path)?;
    }
    fs::rename(temporary, path)?;
    Ok(())
}

pub fn log(event: &RunEvent) -> AppResult<()> {
    const LOG_LIMIT: usize = 1000;
    let path = data_dir()?.join("events.jsonl");
    let line = serde_json::to_string(event)?;
    if !path.exists() {
        let mut file = OpenOptions::new().create(true).append(true).open(path)?;
        writeln!(file, "{line}")?;
        return Ok(());
    }
    let existing = fs::read_to_string(&path)?;
    let mut lines: Vec<&str> = existing.lines().collect();
    lines.push(&line);
    if lines.len() > LOG_LIMIT {
        let remove = lines.len() - LOG_LIMIT;
        lines.drain(..remove);
    }
    fs::write(path, format!("{}\n", lines.join("\n")))?;
    Ok(())
}

pub fn updated(state: &mut ProgressState) {
    state.updated_at = Some(now());
}
