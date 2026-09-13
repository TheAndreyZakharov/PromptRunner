use crate::bank;
use crate::error::{AppError, AppResult};
use crate::input;
use crate::model::{Profile, ProgressSnapshot, ProgressState, RunConfig, RunEvent};
use crate::progress;
use crate::prompt;
use crate::visual::{self, GenerationState};
use crate::writer;
use arboard::Clipboard;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Emitter};

#[derive(Clone)]
pub struct RunnerControl(pub Arc<Mutex<ControlState>>);

#[derive(Debug, Default)]
pub struct ControlState {
    pub running: bool,
    pub paused: bool,
    pub stop: bool,
}

pub fn start(app: AppHandle, control: RunnerControl, config: RunConfig) -> AppResult<()> {
    {
        let mut state = control
            .0
            .lock()
            .map_err(|_| AppError::message("Не удалось получить состояние запуска"))?;
        if state.running {
            return Err(AppError::message("Запуск уже выполняется"));
        }
        state.running = true;
        state.paused = false;
        state.stop = false;
    }
    thread::spawn(move || {
        let result = run_loop(&app, &control, &config);
        if let Err(error) = result {
            let mut saved = progress::load().unwrap_or_default();
            saved.status = if error.to_string().contains("остановлен пользователем")
            {
                "STOPPED"
            } else {
                "ERROR"
            }
            .into();
            progress::updated(&mut saved);
            let _ = progress::save(&saved);
            emit(
                &app,
                event(
                    &saved.status,
                    error.to_string(),
                    saved.current_question_id.clone(),
                    saved.processed,
                    saved.session_limit,
                    saved.session_started_at.clone(),
                    0,
                    Some(error.to_string()),
                ),
            );
        }
        if let Ok(mut state) = control.0.lock() {
            state.running = false;
            state.paused = false;
            state.stop = false;
        }
    });
    Ok(())
}

pub fn pause(control: &RunnerControl) {
    if let Ok(mut state) = control.0.lock() {
        state.paused = true;
    }
}
pub fn resume(control: &RunnerControl) {
    if let Ok(mut state) = control.0.lock() {
        state.paused = false;
    }
}
pub fn toggle_pause(control: &RunnerControl) {
    if let Ok(mut state) = control.0.lock() {
        if state.running {
            state.paused = !state.paused;
        }
    }
}
pub fn stop(control: &RunnerControl) {
    if let Ok(mut state) = control.0.lock() {
        state.stop = true;
        state.paused = false;
    }
}

fn run_loop(app: &AppHandle, control: &RunnerControl, config: &RunConfig) -> AppResult<()> {
    let mut runtime_config = config.clone();
    visual::apply_default_templates(&mut runtime_config.profile, "dark");
    let config = &runtime_config;
    validate_config(config)?;
    let root = Path::new(&config.bank_root);
    let current_fingerprint = bank::fingerprint(root, &config.active_language)?;
    let mut progress_state = progress::load()?;
    if progress_state.bank_root == config.bank_root
        && progress_state.active_language == config.active_language
        && !progress_state.bank_fingerprint.is_empty()
        && progress_state.bank_fingerprint != current_fingerprint
    {
        return Err(AppError::message(
            "Банк вопросов изменился после прошлого запуска; проверьте его и запустите явно заново",
        ));
    }
    let questions = bank::load_questions(root, &config.active_language)?;
    let mut answered_ids = bank::answered_question_ids(&questions)?;
    if let Some(start_id) = config.start_id.as_deref() {
        if !questions.iter().any(|question| question.id == start_id) {
            return Err(AppError::message(format!(
                "Стартовый ID {} не найден среди вопросов {}",
                start_id, config.active_language
            )));
        }
    }
    let saved_last_id = progress_state.last_successful_id.clone();
    let same_run_scope = progress_state.bank_root == config.bank_root
        && progress_state.active_language == config.active_language;
    let resume_id = config
        .start_id
        .clone()
        .or(if same_run_scope { saved_last_id } else { None });
    let mut current = bank::next_question(&questions, resume_id.as_deref())?;
    let started = Instant::now();
    let started_at = Some(format!(
        "{}",
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    ));
    let mut processed = 0_u32;
    progress_state = ProgressState {
        bank_root: config.bank_root.clone(),
        active_language: config.active_language.clone(),
        bank_fingerprint: current_fingerprint,
        session_started_at: started_at.clone(),
        current_question_id: current.as_ref().map(|q| q.id.clone()),
        last_successful_id: if config.start_id.is_none() && same_run_scope {
            resume_id.clone()
        } else {
            None
        },
        processed,
        session_limit: config.session_limit,
        status: "RUNNING".into(),
        updated_at: None,
    };
    progress::updated(&mut progress_state);
    progress::save(&progress_state)?;
    emit(
        app,
        event_with_progress(
            "RUNNING",
            "Сессия запущена",
            current.as_ref().map(|q| q.id.clone()),
            processed,
            config.session_limit,
            started_at.clone(),
            0,
            None,
            bank::progress_snapshot(
                &questions,
                &answered_ids,
                current.as_ref(),
                processed,
                config.session_limit,
            ),
        ),
    );

    while let Some(question) = current {
        check_control(control)?;
        if config
            .session_limit
            .map(|limit| processed >= limit)
            .unwrap_or(false)
        {
            emit(
                app,
                event_with_progress(
                    "SESSION_LIMIT_REACHED",
                    "Достигнут лимит вопросов сессии",
                    Some(question.id.clone()),
                    processed,
                    config.session_limit,
                    started_at.clone(),
                    started.elapsed().as_secs(),
                    None,
                    bank::progress_snapshot(
                        &questions,
                        &answered_ids,
                        Some(&question),
                        processed,
                        config.session_limit,
                    ),
                ),
            );
            progress_state.status = "SESSION_LIMIT_REACHED".into();
            progress::updated(&mut progress_state);
            progress::save(&progress_state)?;
            return Ok(());
        }
        if processed > 0 && config.min_pause_seconds > 0 {
            emit(
                app,
                event(
                    "COOLDOWN",
                    format!("Пауза между запросами: {} сек.", config.min_pause_seconds),
                    Some(question.id.clone()),
                    processed,
                    config.session_limit,
                    started_at.clone(),
                    started.elapsed().as_secs(),
                    None,
                ),
            );
            sleep_with_control(control, Duration::from_secs(config.min_pause_seconds))?;
        }
        emit(
            app,
            event_with_progress(
                "PREPARING",
                "Формируется промпт",
                Some(question.id.clone()),
                processed,
                config.session_limit,
                started_at.clone(),
                started.elapsed().as_secs(),
                None,
                bank::progress_snapshot(
                    &questions,
                    &answered_ids,
                    Some(&question),
                    processed,
                    config.session_limit,
                ),
            ),
        );
        progress_state.current_question_id = Some(question.id.clone());
        progress::updated(&mut progress_state);
        progress::save(&progress_state)?;
        let prompt_text = prompt::build(&crate::model::PromptRequest {
            bank_root: config.bank_root.clone(),
            active_language: config.active_language.clone(),
            question_id: question.id.clone(),
        })?;
        let expected_tail = format!("{} [id: {}]", question.text, question.id);
        if !prompt_text.trim_end().ends_with(&expected_tail) {
            return Err(AppError::message(format!(
                "Сформированный промпт не соответствует текущему вопросу {}",
                question.id
            )));
        }
        if !config.dry_run {
            let input_zone = required_zone(&config.profile, "input")?;
            let send_zone = required_zone(&config.profile, "send")?;
            let copy_zone = required_zone(&config.profile, "copy")?;
            let scroll_zone = required_zone(&config.profile, "scroll")?;
            let observation_zone = required_zone(&config.profile, "generation_state")?;
            let origin = (config.profile.screen_x, config.profile.screen_y);
            let previous_clipboard = if config.preserve_clipboard {
                Clipboard::new()
                    .ok()
                    .and_then(|mut value| value.get_text().ok())
            } else {
                None
            };
            input::paste_text(input_zone, &prompt_text, origin)?;
            wait_until_send_ready(
                app,
                control,
                observation_zone,
                &config.profile,
                &question.id,
                processed,
                config.session_limit,
                &started,
                &started_at,
            )?;
            let busy_seen = click_until_busy(
                app,
                control,
                send_zone,
                observation_zone,
                &config.profile,
                &question.id,
                processed,
                config.session_limit,
                &started,
                &started_at,
            )?;
            wait_until_generation_complete(
                app,
                control,
                observation_zone,
                &config.profile,
                &question.id,
                processed,
                config.session_limit,
                &started,
                &started_at,
                busy_seen,
                config.ready_confirmations,
                scroll_zone,
            )?;
            scroll_after_completion(
                control,
                scroll_zone,
                origin,
                config.profile.scroll_after_seconds,
            )?;
            let sentinel = format!("PROMPTRUNNER_COPY_SENTINEL_{}", question.id);
            let mut clipboard = Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
            clipboard
                .set_text(sentinel.clone())
                .map_err(|e| AppError::Clipboard(e.to_string()))?;
            let mut copied = None;
            for attempt in 0..config.profile.click_retries.max(1) {
                check_control(control)?;
                input::click_zone(copy_zone, attempt, origin)?;
                thread::sleep(Duration::from_millis(500));
                let value = clipboard
                    .get_text()
                    .map_err(|e| AppError::Clipboard(e.to_string()))?;
                if value.trim() != sentinel {
                    copied = Some(value);
                    break;
                }
                if attempt + 1 < config.profile.click_retries.max(1) {
                    emit(
                        app,
                        event(
                            "RETRY_COPY",
                            format!(
                                "Буфер не изменился, повтор копирования ({}/{})",
                                attempt + 1,
                                config.profile.click_retries.max(1)
                            ),
                            Some(question.id.clone()),
                            processed,
                            config.session_limit,
                            started_at.clone(),
                            started.elapsed().as_secs(),
                            None,
                        ),
                    );
                }
            }
            let copied = copied.ok_or_else(|| {
                AppError::message(format!(
                    "Кнопка копирования не изменила буфер для {}",
                    question.id
                ))
            })?;
            writer::replace_question(&question, &copied)?;
            answered_ids.insert(question.id.clone());
            if let Some(previous) = previous_clipboard {
                clipboard
                    .set_text(previous)
                    .map_err(|e| AppError::Clipboard(e.to_string()))?;
            }
            if config.profile.new_chat_every > 0
                && (processed + 1) % config.profile.new_chat_every == 0
            {
                input::new_chat(required_zone(&config.profile, "new_chat")?, origin)?;
                wait_until_empty(
                    app,
                    control,
                    observation_zone,
                    &config.profile,
                    &question.id,
                    processed,
                    config.session_limit,
                    &started,
                    &started_at,
                )?;
            }
        } else {
            emit(
                app,
                event(
                    "DRY_RUN",
                    "Вопрос проверен без управления внешним чатом",
                    Some(question.id.clone()),
                    processed,
                    config.session_limit,
                    started_at.clone(),
                    started.elapsed().as_secs(),
                    None,
                ),
            );
        }
        processed += 1;
        progress_state.current_question_id = None;
        progress_state.last_successful_id = Some(question.id.clone());
        progress_state.processed = processed;
        progress_state.status = "QUESTION_DONE".into();
        progress::updated(&mut progress_state);
        progress::save(&progress_state)?;
        emit(
            app,
            event_with_progress(
                "QUESTION_DONE",
                "Вопрос обработан",
                Some(question.id.clone()),
                processed,
                config.session_limit,
                started_at.clone(),
                started.elapsed().as_secs(),
                None,
                bank::progress_snapshot(
                    &questions,
                    &answered_ids,
                    Some(&question),
                    processed,
                    config.session_limit,
                ),
            ),
        );
        current = bank::next_question_after(&questions, &answered_ids, &question.id);
    }
    progress_state.current_question_id = None;
    progress_state.status = "COMPLETED".into();
    progress::updated(&mut progress_state);
    progress::save(&progress_state)?;
    emit(
        app,
        event_with_progress(
            "COMPLETED",
            "Незакрытых вопросов не осталось",
            None,
            processed,
            config.session_limit,
            started_at,
            started.elapsed().as_secs(),
            None,
            bank::progress_snapshot(
                &questions,
                &answered_ids,
                None,
                processed,
                config.session_limit,
            ),
        ),
    );
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn wait_until_send_ready(
    app: &AppHandle,
    control: &RunnerControl,
    zone: &crate::model::Zone,
    profile: &Profile,
    id: &str,
    processed: u32,
    limit: Option<u32>,
    started: &Instant,
    started_at: &Option<String>,
) -> AppResult<()> {
    input::move_cursor_away(
        zone,
        (profile.screen_x, profile.screen_y),
        profile.screen_width,
        profile.screen_height,
    )?;
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        check_control(control)?;
        match visual::detect(zone, profile.visual_threshold)? {
            GenerationState::SendReady => {
                emit(
                    app,
                    event(
                        "PROMPT_READY",
                        "Промпт находится в поле и готов к отправке",
                        Some(id.to_string()),
                        processed,
                        limit,
                        started_at.clone(),
                        started.elapsed().as_secs(),
                        None,
                    ),
                );
                return Ok(());
            }
            GenerationState::Empty => {}
            GenerationState::Busy => {
                return Err(AppError::message(
                    "До отправки observation-зона уже находится в состоянии writing",
                ))
            }
            GenerationState::Unknown => {
                return Err(AppError::message(
                    "После вставки промпта observation-зона не распознана как send",
                ))
            }
        }
        if Instant::now() >= deadline {
            return Err(AppError::message(format!(
                "После вставки промпт {} не перешёл в состояние send",
                id
            )));
        }
        thread::sleep(Duration::from_millis(250));
    }
}

#[allow(clippy::too_many_arguments)]
fn click_until_busy(
    app: &AppHandle,
    control: &RunnerControl,
    send_zone: &crate::model::Zone,
    observation_zone: &crate::model::Zone,
    profile: &Profile,
    id: &str,
    processed: u32,
    limit: Option<u32>,
    started: &Instant,
    started_at: &Option<String>,
) -> AppResult<bool> {
    for attempt in 0..profile.click_retries.max(1) {
        check_control(control)?;
        input::click_zone(send_zone, attempt, (profile.screen_x, profile.screen_y))?;
        // После отправки курсор не должен закрывать или менять observation-зону.
        input::move_cursor_away(
            observation_zone,
            (profile.screen_x, profile.screen_y),
            profile.screen_width,
            profile.screen_height,
        )?;
        let deadline =
            Instant::now() + Duration::from_secs(profile.send_retry_delay_seconds.max(1));
        loop {
            match visual::detect(observation_zone, profile.visual_threshold)? {
                GenerationState::Busy => {
                    emit(
                        app,
                        event(
                            "SUBMITTED",
                            format!("Промпт отправлен, попытка {}", attempt + 1),
                            Some(id.to_string()),
                            processed,
                            limit,
                            started_at.clone(),
                            started.elapsed().as_secs(),
                            None,
                        ),
                    );
                    return Ok(true);
                }
                GenerationState::SendReady => {
                    // Keep the prompt in the input and wait the configured
                    // retry delay before clicking send again. This avoids a
                    // burst of clicks when the chat UI has not reacted yet.
                }
                GenerationState::Empty => {
                    if Instant::now() >= deadline {
                        // Empty can mean that the click was swallowed by the
                        // chat. Treat it like send-ready and retry the same
                        // prompt instead of failing immediately.
                    }
                }
                GenerationState::Unknown => {
                    return Err(AppError::message(
                        "После отправки observation-зона не распознана",
                    ))
                }
            }
            if Instant::now() >= deadline {
                emit(
                    app,
                    event(
                        "RETRY_SEND",
                        format!(
                            "Writing не подтверждён, повторная отправка после ожидания {} сек. ({}/{})",
                            profile.send_retry_delay_seconds.max(1),
                            attempt + 1,
                            profile.click_retries.max(1)
                        ),
                        Some(id.to_string()),
                        processed,
                        limit,
                        started_at.clone(),
                        started.elapsed().as_secs(),
                        None,
                    ),
                );
                break;
            }
            thread::sleep(Duration::from_millis(250));
        }
    }
    Err(AppError::message(format!(
        "Не удалось подтвердить отправку промпта для {}",
        id
    )))
}

#[allow(clippy::too_many_arguments)]
fn wait_until_generation_complete(
    app: &AppHandle,
    control: &RunnerControl,
    zone: &crate::model::Zone,
    profile: &Profile,
    id: &str,
    processed: u32,
    limit: Option<u32>,
    started: &Instant,
    started_at: &Option<String>,
    mut busy_seen: bool,
    confirmations_required: u8,
    scroll_zone: &crate::model::Zone,
) -> AppResult<()> {
    input::move_cursor_away(
        zone,
        (profile.screen_x, profile.screen_y),
        profile.screen_width,
        profile.screen_height,
    )?;
    let deadline = Instant::now() + Duration::from_secs(profile.generation_timeout_seconds.max(1));
    let mut empty_seen = 0_u8;
    let mut next_scroll = Instant::now();
    let mut scroll_attempt = 0_u8;
    let scroll_interval = Duration::from_secs(1);
    let poll_interval = Duration::from_secs(profile.poll_interval_seconds.max(1));
    let mut next_detection = Instant::now();
    loop {
        check_control(control)?;
        if Instant::now() > deadline {
            return Err(AppError::message(format!(
                "Истёк тайм-аут генерации для {}",
                id
            )));
        }
        if busy_seen && Instant::now() >= next_scroll {
            input::scroll_down(
                scroll_zone,
                scroll_attempt,
                (profile.screen_x, profile.screen_y),
            )?;
            scroll_attempt = scroll_attempt.wrapping_add(1);
            next_scroll = Instant::now() + scroll_interval;
        }
        if Instant::now() < next_detection {
            thread::sleep(Duration::from_millis(100));
            continue;
        }
        next_detection = Instant::now() + poll_interval;
        match visual::detect(zone, profile.visual_threshold)? {
            GenerationState::Busy => {
                if !busy_seen {
                    busy_seen = true;
                    next_scroll = Instant::now();
                }
                empty_seen = 0;
                emit(
                    app,
                    event(
                        "WAITING",
                        "AI-чат ещё генерирует ответ",
                        Some(id.to_string()),
                        processed,
                        limit,
                        started_at.clone(),
                        started.elapsed().as_secs(),
                        None,
                    ),
                );
            }
            GenerationState::Empty => {
                if !busy_seen {
                    emit(
                        app,
                        event(
                            "WAITING_FOR_BUSY",
                            "Отправка ещё не подтверждена переходом в состояние генерации",
                            Some(id.to_string()),
                            processed,
                            limit,
                            started_at.clone(),
                            started.elapsed().as_secs(),
                            None,
                        ),
                    );
                    continue;
                }
                empty_seen += 1;
                emit(
                    app,
                    event(
                        "READY_CHECK",
                        format!("Состояние no_prompt подтверждено ({empty_seen}/{confirmations_required})"),
                        Some(id.to_string()),
                        processed,
                        limit,
                        started_at.clone(),
                        started.elapsed().as_secs(),
                        None,
                    ),
                );
                if empty_seen >= confirmations_required.max(1) {
                    return Ok(());
                }
            }
            GenerationState::SendReady => {
                emit(
                    app,
                    event(
                        "WAITING",
                        if busy_seen {
                            "Ответ ещё не перешёл в пустое состояние поля ввода"
                        } else {
                            "Промпт готов к отправке, ожидание начала генерации"
                        },
                        Some(id.to_string()),
                        processed,
                        limit,
                        started_at.clone(),
                        started.elapsed().as_secs(),
                        None,
                    ),
                );
                empty_seen = 0;
            }
            GenerationState::Unknown => {
                emit(
                    app,
                    event(
                        "PAUSED_AMBIGUOUS",
                        "Не удалось уверенно определить состояние observation-зоны",
                        Some(id.to_string()),
                        processed,
                        limit,
                        started_at.clone(),
                        started.elapsed().as_secs(),
                        None,
                    ),
                );
                return Err(AppError::message("Неоднозначное состояние генерации"));
            }
        }
        thread::sleep(Duration::from_millis(100));
    }
}

fn scroll_after_completion(
    control: &RunnerControl,
    zone: &crate::model::Zone,
    origin: (i32, i32),
    seconds: u64,
) -> AppResult<()> {
    if seconds == 0 {
        return Ok(());
    }
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let mut attempt = 0_u8;
    while Instant::now() < deadline {
        check_control(control)?;
        input::scroll_down(zone, attempt, origin)?;
        attempt = attempt.wrapping_add(1);
        let remaining = deadline.saturating_duration_since(Instant::now());
        thread::sleep(remaining.min(Duration::from_millis(500)));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn wait_until_empty(
    app: &AppHandle,
    control: &RunnerControl,
    zone: &crate::model::Zone,
    profile: &Profile,
    id: &str,
    processed: u32,
    limit: Option<u32>,
    started: &Instant,
    started_at: &Option<String>,
) -> AppResult<()> {
    input::move_cursor_away(
        zone,
        (profile.screen_x, profile.screen_y),
        profile.screen_width,
        profile.screen_height,
    )?;
    let deadline =
        Instant::now() + Duration::from_secs(profile.generation_timeout_seconds.clamp(1, 30));
    let mut confirmations = 0_u8;
    while Instant::now() <= deadline {
        check_control(control)?;
        if visual::detect_empty(zone, profile.visual_threshold)? {
            confirmations += 1;
            emit(
                app,
                event(
                    "NEW_CHAT_READY",
                    format!("Новый чат подтверждён по empty-шаблону ({confirmations}/2)"),
                    Some(id.to_string()),
                    processed,
                    limit,
                    started_at.clone(),
                    started.elapsed().as_secs(),
                    None,
                ),
            );
            if confirmations >= 2 {
                return Ok(());
            }
        } else {
            confirmations = 0;
            emit(
                app,
                event(
                    "WAITING_NEW_CHAT",
                    "Ожидание визуального подтверждения нового пустого чата",
                    Some(id.to_string()),
                    processed,
                    limit,
                    started_at.clone(),
                    started.elapsed().as_secs(),
                    None,
                ),
            );
        }
        thread::sleep(Duration::from_millis(500));
    }
    Err(AppError::message(
        "Не удалось визуально подтвердить открытие нового пустого чата",
    ))
}

fn required_zone<'a>(profile: &'a Profile, name: &str) -> AppResult<&'a crate::model::Zone> {
    profile
        .zones
        .iter()
        .find(|zone| {
            zone.name == name
                && zone.kind
                    == if name == "generation_state" {
                        "observation"
                    } else {
                        "action"
                    }
        })
        .ok_or_else(|| AppError::message(format!("В профиле отсутствует зона {name}")))
}

fn validate_config(config: &RunConfig) -> AppResult<()> {
    if !config
        .allowed_languages
        .iter()
        .any(|value| value == &config.active_language)
    {
        return Err(AppError::message(
            "Активный язык не входит в разрешённый allowlist",
        ));
    }
    if config.allowed_languages.is_empty() {
        return Err(AppError::message("Не выбран ни один разрешённый язык"));
    }
    if !config.dry_run {
        if config.profile.zones.is_empty() {
            return Err(AppError::message(
                "Перед запуском нужен сохранённый профиль зон",
            ));
        }
        let screen = visual::screens()?
            .into_iter()
            .find(|item| item.id == config.profile.screen_id)
            .ok_or_else(|| {
                AppError::message("Монитор профиля недоступен; откалибруйте профиль заново")
            })?;
        if screen.width != config.profile.screen_width
            || screen.height != config.profile.screen_height
            || (screen.scale_factor - config.profile.scale_factor).abs() > 0.01
        {
            return Err(AppError::message("Разрешение или масштаб монитора изменились; выберите или создайте подходящий профиль"));
        }
        for name in [
            "input",
            "send",
            "copy",
            "new_chat",
            "scroll",
            "generation_state",
        ] {
            let kind = if name == "generation_state" {
                "observation"
            } else {
                "action"
            };
            let zone = required_zone(&config.profile, name)?;
            if zone.kind != kind || !zone.is_valid() {
                return Err(AppError::message(format!("Зона {name} некорректна")));
            }
        }
        let state_zone = required_zone(&config.profile, "generation_state")?;
        for (label, path) in [
            ("busy", state_zone.sample_busy.as_deref()),
            ("send", state_zone.sample_ready.as_deref()),
            ("no_prompt", state_zone.sample_empty.as_deref()),
        ] {
            let path = path.ok_or_else(|| {
                AppError::message(format!("Не указан {label}-шаблон observation-зоны"))
            })?;
            if !Path::new(path).is_file() {
                return Err(AppError::message(format!(
                    "{label}-шаблон не найден: {path}"
                )));
            }
        }
        input::current_pointer()?;
    }
    Ok(())
}

fn check_control(control: &RunnerControl) -> AppResult<()> {
    loop {
        let state = control
            .0
            .lock()
            .map_err(|_| AppError::message("Не удалось прочитать состояние запуска"))?;
        if state.stop {
            return Err(AppError::message("Запуск остановлен пользователем"));
        }
        if !state.paused {
            return Ok(());
        }
        drop(state);
        thread::sleep(Duration::from_millis(200));
    }
}

fn sleep_with_control(control: &RunnerControl, duration: Duration) -> AppResult<()> {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        check_control(control)?;
        let remaining = deadline.saturating_duration_since(Instant::now());
        thread::sleep(Duration::from_millis(100).min(remaining));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn event(
    state: &str,
    message: impl Into<String>,
    question_id: Option<String>,
    processed: u32,
    limit: Option<u32>,
    started_at: Option<String>,
    elapsed: u64,
    error: Option<String>,
) -> RunEvent {
    RunEvent {
        state: state.into(),
        message: message.into(),
        question_id,
        processed,
        session_limit: limit,
        started_at,
        elapsed_seconds: elapsed,
        error,
        progress: None,
    }
}

#[allow(clippy::too_many_arguments)]
fn event_with_progress(
    state: &str,
    message: impl Into<String>,
    question_id: Option<String>,
    processed: u32,
    limit: Option<u32>,
    started_at: Option<String>,
    elapsed: u64,
    error: Option<String>,
    progress: ProgressSnapshot,
) -> RunEvent {
    let mut result = event(
        state,
        message,
        question_id,
        processed,
        limit,
        started_at,
        elapsed,
        error,
    );
    result.progress = Some(progress);
    result
}

fn emit(app: &AppHandle, payload: RunEvent) {
    let _ = progress::log(&payload);
    let _ = app.emit("runner:event", payload);
}
