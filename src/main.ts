import { invoke } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import type { CalibrationResult, Profile, ProgressMetric, ProgressState, RunEvent, ScreenCapture, ScreenInfo, Settings, Summary, TemplateDefaults, Zone, ZoneKind } from "./types";
import "./styles.css";

const defaults: Settings = { bank_root: "", allowed_languages: ["RU"], active_language: "RU", poll_interval_seconds: 10, generation_timeout_seconds: 600, click_retries: 3, new_chat_every: 100, min_pause_seconds: 12, ready_confirmations: 2, preserve_clipboard: true, theme: "dark" };
let settings = structuredClone(defaults);
let profiles: Profile[] = [];
let selectedProfile: Profile | null = null;
let summary: Summary | null = null;
let latest: RunEvent | null = null;
let eventLog: RunEvent[] = [];
let sessionLimit = "";
let startId = "";
let dryRun = false;
let view: "home" | "session" | "calibration" | "settings" = "home";
let tab: ZoneKind = "action";
let zones: Zone[] = [];
let screens: ScreenInfo[] = [];
let selectedScreen: ScreenInfo | null = null;
let storedProgress: ProgressState | null = null;
let calibrationDraft: Zone[] = [];
let bankSaveTimer: number | undefined;
let templateDefaults: TemplateDefaults | null = null;
const specs = [
  ["input", "Поле ввода", "paste_prompt", "action"],
  ["send", "Отправка", "submit_prompt", "action"],
  ["copy", "Копирование", "copy_answer", "action"],
  ["new_chat", "Новый чат", "new_chat", "action"],
  ["generation_state", "Состояние генерации", "detect_generation_finished", "observation"]
] as const;

const app = () => document.querySelector<HTMLDivElement>("#app")!;
const esc = (value: string) => value.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#039;" })[c] ?? c);
const duration = (seconds: number) => Math.floor(seconds / 60).toString().padStart(2, "0") + ":" + (seconds % 60).toString().padStart(2, "0");
const percent = (value: number) => `${Math.max(0, Math.min(100, value)).toFixed(value % 1 === 0 ? 0 : 1)}%`;
const LOG_LIMIT = 1000;
const addLog = (item: RunEvent) => { eventLog.unshift(item); if (eventLog.length > LOG_LIMIT) eventLog.length = LOG_LIMIT; };
const progressBar = (label: string, metric: ProgressMetric, className = "") => `<div class="progress-row ${className}"><div class="progress-label"><span>${esc(label)}</span><strong>${metric.completed} / ${metric.total} · ${percent(metric.percent)}</strong></div><div class="progress"><span style="width:${Math.max(0, Math.min(100, metric.percent))}%"></span></div></div>`;
const withTemplateDefaults = (item: Zone): Zone => item.name === "generation_state" ? { ...item, sample_busy: item.sample_busy ?? templateDefaults?.sample_busy, sample_ready: item.sample_ready ?? templateDefaults?.sample_ready, sample_empty: item.sample_empty ?? templateDefaults?.sample_empty } : item;
const zone = (name: string, source: Zone[] = zones): Zone => withTemplateDefaults(source.find((item) => item.name === name) ?? { name, kind: specs.find((item) => item[0] === name)![3], purpose: specs.find((item) => item[0] === name)![2], x: 0, y: 0, width: 0, height: 0, screen_index: 0 });
const setZone = (item: Zone) => { const existing = zones.findIndex((candidate) => candidate.name === item.name); if (existing >= 0) zones[existing] = item; else zones.push(item); if (selectedProfile) { selectedProfile = { ...selectedProfile, zones: structuredClone(zones) }; void invoke("save_profile", { value: selectedProfile }).catch((error) => showError(String(error))); } };

function render() {
  document.documentElement.dataset.theme = settings.theme;
  const event = latest;
  const bankProgress = event?.progress?.bank ?? summary?.progress;
  const currentSection = event?.progress?.current_section ?? summary?.current_section;
  const currentSubtopic = event?.progress?.current_subtopic ?? summary?.current_subtopic;
  const leftVisible = view === "session" || view === "calibration";
  const rightVisible = view === "home" || view === "settings";
  const sectionProgress = currentSection ? progressBar(`Текущий пункт · ${currentSection.name}`, currentSection.progress, "section-progress") : "<div class=\"empty\">Текущий пункт появится после проверки банка или запуска.</div>";
  const subtopicProgress = currentSubtopic ? progressBar(`Текущий подпункт · ${currentSubtopic.name}`, currentSubtopic.progress, "subtopic-progress") : "<div class=\"empty\">Текущий подпункт появится после проверки банка или запуска.</div>";
  const overallProgress = bankProgress ? progressBar("Все вопросы банка", bankProgress.overall, "overall-progress") : "<div class=\"empty\">Проверьте банк, чтобы увидеть прогресс.</div>";
  const shownZones = specs.filter((item) => item[3] === tab).map(([name, label, purpose, kind]) => {
    const item = zone(name);
    return `<div class="zone-card"><div class="zone-head"><strong>${label}</strong><span class="tag">${name}</span></div><div class="zone-fields">${(["x", "y", "width", "height"] as const).map((field) => `<label>${field}<input data-zone="${name}" data-field="${field}" type="number" min="${field === "x" || field === "y" ? 0 : 1}" value="${item[field]}"></label>`).join("")}</div>${kind === "observation" ? `<div class="zone-fields wide"><label>writing-шаблон (busy)<input data-zone="${name}" data-field="sample_busy" value="${esc(item.sample_busy ?? "")}" placeholder="/path/writing_dark.png"></label><label>send-шаблон (готов к отправке)<input data-zone="${name}" data-field="sample_ready" value="${esc(item.sample_ready ?? "")}" placeholder="/path/send_dark.png"></label></div>` : ""}<small>${kind === "observation" ? "writing = генерация идёт; no_prompt = поле пустое и ответ завершён; send = текст готов к отправке. Встроенные шаблоны, включая дополнительные варианты, подставляются автоматически." : "Клик внутри зоны с повторной точкой при необходимости."}</small></div>`;
  }).join("");

  app().innerHTML = `
  <main class="shell">
    <header class="topbar"><div><div class="eyebrow">BLACK-BOX UI TESTER</div><h1>PromptRunner</h1></div><div class="top-actions"><span class="pill ${event?.state?.toLowerCase() ?? "idle"}">${event?.state ?? "READY"}</span><select id="theme"><option value="system">Системная тема</option><option value="light">Светлая тема</option><option value="dark">Тёмная тема</option></select></div></header>
    <nav class="main-nav"><button data-view="home" class="${view === "home" ? "active" : ""}">Главная</button><button data-view="session" class="${view === "session" ? "active" : ""}">Сессия</button><button data-view="calibration" class="${view === "calibration" ? "active" : ""}">Калибровка зон</button><button data-view="settings" class="${view === "settings" ? "active" : ""}">Настройки</button></nav>
    <section class="grid ${view === "home" ? "home-grid" : "single-grid"}"><div class="column left-column ${leftVisible ? "" : "empty-column"}">
      <section class="card ${view === "session" ? "" : "hidden-panel"}"><div class="card-title"><h2>Сессия</h2><span class="muted">управление запуском</span></div>
        <label>Репозиторий банка вопросов<div class="input-with-button"><input id="bank" value="${esc(settings.bank_root)}" placeholder="/Users/.../IT-Interview-Question-Bank"><button id="choose-bank" type="button">Выбрать</button></div></label>
        <div class="row"><label>Разрешены языки<div class="checks"><label class="check"><input id="ru" type="checkbox" ${settings.allowed_languages.includes("RU") ? "checked" : ""}> RU</label><label class="check"><input id="en" type="checkbox" ${settings.allowed_languages.includes("EN") ? "checked" : ""}> EN</label></div></label><label>Активный язык<select id="active"><option value="RU" ${settings.active_language === "RU" ? "selected" : ""} ${settings.allowed_languages.includes("RU") ? "" : "disabled"}>RU</option><option value="EN" ${settings.active_language === "EN" ? "selected" : ""} ${settings.allowed_languages.includes("EN") ? "" : "disabled"}>EN</option></select></label></div>
        <div class="row"><label>Начать с ID<input id="start" value="${esc(startId)}" placeholder="первый незакрытый"></label><label>Вопросов за сессию<input id="limit" type="number" min="1" value="${esc(sessionLimit)}" placeholder="без ограничения"></label></div>
        <div class="row"><label>Профиль<select id="profile"><option value="">нет профиля — сначала калибровка</option>${profiles.map((item) => `<option value="${item.id}" ${selectedProfile?.id === item.id ? "selected" : ""}>${esc(item.name)} · ${item.platform}</option>`).join("")}</select></label><label class="switchline"><input id="dry" type="checkbox" ${dryRun ? "checked" : ""}> Проверочный прогон без кликов</label></div>
        <div class="button-row"><button id="check" class="secondary">Проверить банк</button><button id="preview" class="secondary">Предпросмотр промпта</button><button id="start-run" class="primary">Старт</button><button id="pause" class="secondary">Пауза</button><button id="resume" class="secondary">Продолжить</button><button id="stop" class="danger">Стоп</button></div><details class="prompt-preview"><summary>Итоговый промпт</summary><pre id="prompt-preview">Введите стартовый ID и нажмите «Предпросмотр промпта».</pre></details>
        <div class="summary">${summary ? `<strong>${summary.unanswered_questions}</strong> незакрытых из ${summary.total_questions} · ответов записано ${summary.answered_questions} · следующий ID: <strong>${summary.next_question_id ?? "— всё отвечено"}</strong>` : "Банк ещё не проверен"}</div>
      </section>
      <section class="card ${view === "calibration" ? "" : "hidden-panel"}"><div class="card-title"><h2>Калибровка зон</h2><span class="muted">готовых профилей нет</span></div><p class="hint">Откройте AI-чат, зафиксируйте окно и внесите прямоугольники. После калибровки PromptRunner автоматически сохранит профиль на диске.</p><div class="zone-tabs"><button data-tab="action" class="${tab === "action" ? "active" : ""}">Действия</button><button data-tab="observation" class="${tab === "observation" ? "active" : ""}">Наблюдение</button></div><div>${shownZones || '<div class="empty">Нет зон этого типа.</div>'}</div><div class="profile-actions"><input id="profile-name" value="${esc(selectedProfile?.name ?? "")}" placeholder="Название профиля"><button id="pointer" class="secondary">Позиция курсора</button><button id="save-profile" class="secondary">Сохранить профиль</button></div><div id="pointer-status" class="hint"></div></section>
    </div><div class="column right-column ${rightVisible ? "" : "empty-column"}">
      <section class="card telemetry ${view === "home" ? "" : "hidden-panel"}"><div class="card-title"><h2>Прогресс</h2><span class="muted">банк · текущий пункт · текущий подпункт</span></div><div class="metrics"><div><span>Текущий ID</span><strong>${event?.question_id ?? "—"}</strong></div><div><span>Обработано</span><strong>${event?.processed ?? 0}${event?.session_limit ? ` / ${event.session_limit}` : ""}</strong></div><div><span>Время работы</span><strong>${duration(event?.elapsed_seconds ?? 0)}</strong></div></div><div class="progress-stack">${overallProgress}${sectionProgress}${subtopicProgress}</div><p class="status-message">${esc(event?.message ?? "Готов к настройке")}</p><div class="home-run-controls"><button id="home-start-run" class="primary">Старт</button><button id="home-pause-run" class="secondary">Пауза / продолжить</button></div></section>
      <section class="card ${view === "settings" ? "" : "hidden-panel"}"><div class="card-title"><h2>Настройки ожидания</h2><span class="muted">применяются при старте</span></div><div class="row"><label>Проверка состояния, сек<input id="poll" type="number" min="1" value="${settings.poll_interval_seconds}"></label><label>Тайм-аут генерации, сек<input id="timeout" type="number" min="10" value="${settings.generation_timeout_seconds}"></label></div><div class="row"><label>Пауза между запросами, сек<input id="cooldown" type="number" min="0" value="${settings.min_pause_seconds}"></label><label>Подтверждений no_prompt<input id="confirmations" type="number" min="1" max="5" value="${settings.ready_confirmations}"></label></div><div class="row"><label>Попыток отправить после вставки<input id="retries" type="number" min="1" max="8" value="${settings.click_retries}"></label><label>Новый чат каждые N вопросов<input id="new-chat" type="number" min="0" value="${settings.new_chat_every}"></label></div><label class="switchline"><input id="preserve-clipboard" type="checkbox" ${settings.preserve_clipboard ? "checked" : ""}> Восстанавливать прежний буфер после записи ответа</label><p class="hint">После вставки приложение ждёт состояние send и делает заданное число попыток клика отправки. Состояние генерации должно подтвердиться отдельно.</p></section>
      <section class="card log-card ${view === "home" ? "" : "hidden-panel"}"><div class="card-title"><h2>Лог</h2><button id="clear" class="link">Очистить</button></div><pre>${eventLog.length ? eventLog.map((item) => `[${item.state}] ${item.question_id ?? "—"} · ${item.message}${item.error ? `\nОшибка: ${item.error}` : ""}`).join("\n") : "События появятся после запуска."}</pre></section>
    </div></section><footer><span>Координатное управление · без DOM/API тестируемого чата</span><span>PromptRunner 0.1.0</span></footer>
  </main>`;
  wire();
}

function wire() {
  if (!latest && storedProgress?.last_successful_id) {
    const status = document.querySelector(".status-message");
    if (status) status.textContent = "Сохранённый прогресс: последний успешный ID " + storedProgress.last_successful_id + ". Следующий запуск продолжит дальше.";
  }
  (document.querySelector("#theme") as HTMLSelectElement).value = settings.theme;
  (document.querySelector("#active") as HTMLSelectElement).value = settings.active_language;
  document.querySelectorAll<HTMLButtonElement>("[data-view]").forEach((button) => button.addEventListener("click", () => { view = button.dataset.view as typeof view; render(); }));
  document.querySelector("#theme")!.addEventListener("change", (e) => { settings.theme = (e.target as HTMLSelectElement).value as Settings["theme"]; saveSettings(); render(); });
  document.querySelector("#active")!.addEventListener("change", (e) => { settings.active_language = (e.target as HTMLSelectElement).value; startId = ""; summary = null; saveSettings(); render(); if (settings.bank_root) void validate(); });
  document.querySelector("#profile")!.addEventListener("change", async (e) => { const id = (e.target as HTMLSelectElement).value; const nextProfile = id ? structuredClone(profiles.find((item) => item.id === id) ?? null) : null; selectedProfile = nextProfile; zones = nextProfile ? structuredClone(nextProfile.zones) : []; selectedScreen = nextProfile ? screens.find((item) => item.index === nextProfile.screen_index) ?? selectedScreen : selectedScreen; settings.selected_profile_id = nextProfile?.id; await saveSettings(); render(); });
  const rememberBankPath = (event: Event) => { settings.bank_root = (event.target as HTMLInputElement).value.trim(); window.clearTimeout(bankSaveTimer); bankSaveTimer = window.setTimeout(() => { void saveSettings(); }, 300); };
  document.querySelector("#bank")!.addEventListener("input", rememberBankPath);
  document.querySelector("#bank")!.addEventListener("change", (e) => { rememberBankPath(e); startId = ""; summary = null; render(); if (settings.bank_root) void validate(); });
  document.querySelector("#bank")!.addEventListener("blur", () => { window.clearTimeout(bankSaveTimer); void saveSettings(); });
  document.querySelector("#choose-bank")!.addEventListener("click", chooseBank);
  document.querySelector("#ru")!.addEventListener("change", updateLanguages); document.querySelector("#en")!.addEventListener("change", updateLanguages);
  document.querySelector("#start")!.addEventListener("change", (e) => { startId = (e.target as HTMLInputElement).value.trim(); });
  document.querySelector("#limit")!.addEventListener("change", (e) => { sessionLimit = (e.target as HTMLInputElement).value; });
  document.querySelector("#dry")!.addEventListener("change", (e) => { dryRun = (e.target as HTMLInputElement).checked; });
  for (const id of ["poll", "timeout", "cooldown", "confirmations", "retries", "new-chat"]) document.querySelector("#" + id)!.addEventListener("change", updateTiming);
  document.querySelector("#preserve-clipboard")!.addEventListener("change", (e) => { settings.preserve_clipboard = (e.target as HTMLInputElement).checked; saveSettings(); });
  document.querySelector("#check")!.addEventListener("click", validate);
  document.querySelector("#preview")!.addEventListener("click", previewPrompt);
  document.querySelector("#start-run")?.addEventListener("click", startRun);
  document.querySelector("#home-start-run")?.addEventListener("click", startRun);
  document.querySelector("#pause")!.addEventListener("click", () => invoke("pause_run"));
  document.querySelector("#resume")!.addEventListener("click", () => invoke("resume_run"));
  document.querySelector("#home-pause-run")?.addEventListener("click", () => invoke("toggle_pause").catch((error) => showError(String(error))));
  document.querySelector("#stop")!.addEventListener("click", () => invoke("stop_run"));
  document.querySelector("#pointer")!.addEventListener("click", pointer);
  const calibrationButton = document.createElement("button");
  calibrationButton.className = "secondary calibration-button";
  calibrationButton.textContent = "Открыть разметку поверх экрана";
  document.querySelector(".zone-tabs")?.before(calibrationButton);
  calibrationButton.addEventListener("click", () => invoke("open_calibration", { screenIndex: selectedScreen?.index ?? 0 }).catch((error) => showError(String(error))));
  document.querySelector("#save-profile")!.addEventListener("click", saveProfile);
  document.querySelector("#clear")!.addEventListener("click", () => { latest = null; eventLog = []; render(); });
  document.querySelectorAll<HTMLButtonElement>("[data-tab]").forEach((button) => button.addEventListener("click", () => { tab = button.dataset.tab as ZoneKind; render(); }));
  document.querySelectorAll<HTMLInputElement>("[data-zone]").forEach((input) => input.addEventListener("change", () => {
    const item = { ...zone(input.dataset.zone!), [input.dataset.field!]: input.dataset.field!.startsWith("sample") ? input.value : Number(input.value) } as Zone;
    setZone(item);
  }));
  if (tab === "action") {
    document.querySelectorAll(".zone-card").forEach((card) => {
      const name = card.querySelector<HTMLInputElement>("[data-zone]")?.dataset.zone;
      if (!name) return;
      const button = document.createElement("button");
      button.className = "zone-test";
      button.textContent = "Проверить клик";
      button.addEventListener("click", () => { if (!window.confirm(`Выполнить тестовый клик по зоне «${name}»? Убедитесь, что AI-чат открыт.`)) return; invoke("test_action_zone", { zone: zone(name), originX: selectedScreen?.x ?? 0, originY: selectedScreen?.y ?? 0 }).catch((error) => showError(String(error))); });
      card.append(button);
    });
  }
  if (tab === "observation") {
    const card = document.querySelector(".zone-card");
    if (card) {
      const actions = document.createElement("div");
      actions.className = "template-actions";
      actions.innerHTML = '<input class="empty-template" data-zone="generation_state" data-field="sample_empty" value="' + esc(zone("generation_state").sample_empty ?? "") + '" placeholder="/path/no_prompt_dark.png"><button type="button" data-template="sample_busy">Снять writing / busy</button><button type="button" data-template="sample_ready">Снять send</button><button type="button" data-template="sample_empty">Снять no_prompt</button>';
      card.append(actions);
      actions.querySelector<HTMLInputElement>('[data-field="sample_empty"]')?.addEventListener("change", (event) => setZone({ ...zone("generation_state"), sample_empty: (event.target as HTMLInputElement).value.trim() }));
      actions.querySelectorAll<HTMLButtonElement>("[data-template]").forEach((button) => button.addEventListener("click", () => captureTemplate(button.dataset.template as "sample_busy" | "sample_ready" | "sample_empty")));
    }
  }
}

function updateLanguages() { settings.allowed_languages = ["RU", "EN"].filter((language) => (document.querySelector("#" + language.toLowerCase()) as HTMLInputElement).checked); if (!settings.allowed_languages.includes(settings.active_language)) settings.active_language = settings.allowed_languages[0] ?? ""; saveSettings(); render(); }
function updateTiming() { settings.poll_interval_seconds = Math.max(1, Number((document.querySelector("#poll") as HTMLInputElement).value) || 10); settings.generation_timeout_seconds = Math.max(10, Number((document.querySelector("#timeout") as HTMLInputElement).value) || 600); settings.min_pause_seconds = Math.max(0, Number((document.querySelector("#cooldown") as HTMLInputElement).value) || 0); settings.ready_confirmations = Math.min(5, Math.max(1, Number((document.querySelector("#confirmations") as HTMLInputElement).value) || 2)); settings.click_retries = Math.min(8, Math.max(1, Number((document.querySelector("#retries") as HTMLInputElement).value) || 3)); settings.new_chat_every = Math.max(0, Number((document.querySelector("#new-chat") as HTMLInputElement).value) || 0); saveSettings(); }
async function captureTemplate(kind: "sample_busy" | "sample_ready" | "sample_empty") {
  try {
    const pathInput = document.querySelector<HTMLInputElement>('[data-zone="generation_state"][data-field="' + kind + '"]');
    const path = pathInput?.value.trim() || window.prompt("Путь для сохранения visual-шаблона")?.trim();
    if (!path) throw new Error("Сначала укажите путь файла для visual-шаблона");
    const item = { ...zone("generation_state"), screen_index: selectedScreen?.index ?? zone("generation_state").screen_index };
    await invoke("save_template", { zone: item, path });
    setZone({ ...item, [kind]: path });
    render();
  } catch (error) { showError(String(error)); }
}
async function saveSettings() { try { await invoke("save_settings", { settings }); } catch (error) { showError(String(error)); } }
async function chooseBank() { try { const path = await invoke<string | null>("choose_bank_directory"); if (path) { settings.bank_root = path; startId = ""; summary = null; await saveSettings(); render(); await validate(); } } catch (error) { showError(String(error)); } }
async function fillPromptPreview() { if (!startId.trim() || !settings.bank_root) return; try { const prompt = await invoke<string>("build_prompt", { request: { bank_root: settings.bank_root, active_language: settings.active_language, question_id: startId.trim() } }); const target = document.querySelector("#prompt-preview"); if (target) target.textContent = prompt; } catch { /* validate already reports a malformed bank */ } }
async function validate() { try { summary = await invoke<Summary>("validate_bank", { root: settings.bank_root, language: settings.active_language }); startId = summary.next_question_id ?? ""; render(); await fillPromptPreview(); } catch (error) { showError(String(error)); } }
async function previewPrompt() { try { const questionId = startId.trim(); if (!questionId) throw new Error("Для предпросмотра укажите стартовый ID"); await fillPromptPreview(); const details = document.querySelector<HTMLDetailsElement>(".prompt-preview"); if (details) details.open = true; } catch (error) { showError(String(error)); } }
async function startRun() {
  try {
    const profile = { ...(selectedProfile ?? { id: "", name: "", platform: "unknown", screen_index: selectedScreen?.index ?? 0, screen_id: selectedScreen?.id ?? 0, screen_x: selectedScreen?.x ?? 0, screen_y: selectedScreen?.y ?? 0, screen_width: selectedScreen?.width ?? 0, screen_height: selectedScreen?.height ?? 0, scale_factor: selectedScreen?.scale_factor ?? 1, zones, visual_threshold: 0.82 }), zones: zones.map(withTemplateDefaults), poll_interval_seconds: settings.poll_interval_seconds, generation_timeout_seconds: settings.generation_timeout_seconds, click_retries: settings.click_retries, new_chat_every: settings.new_chat_every };
    await invoke("start_run", { config: { bank_root: settings.bank_root, active_language: settings.active_language, allowed_languages: settings.allowed_languages, start_id: startId || null, session_limit: sessionLimit ? Number(sessionLimit) : null, min_pause_seconds: settings.min_pause_seconds, ready_confirmations: settings.ready_confirmations, preserve_clipboard: settings.preserve_clipboard, profile, dry_run: dryRun } });
  } catch (error) { showError(String(error)); }
}
async function pointer() { try { const [x, y] = await invoke<[number, number]>("pointer_position"); document.querySelector("#pointer-status")!.textContent = `Текущая позиция: ${x}, ${y}. Внесите координаты в нужную зону.`; } catch (error) { showError(String(error)); } }
function makeProfile(name: string, id?: string): Profile { const screen = selectedScreen; return { id: id ?? crypto.randomUUID(), name, platform: selectedProfile?.platform ?? navigator.platform, screen_index: screen?.index ?? 0, screen_id: screen?.id ?? 0, screen_x: screen?.x ?? 0, screen_y: screen?.y ?? 0, screen_width: screen?.width ?? window.screen.width, screen_height: screen?.height ?? window.screen.height, scale_factor: screen?.scale_factor ?? devicePixelRatio, zones: structuredClone(zones).map(withTemplateDefaults), poll_interval_seconds: settings.poll_interval_seconds, generation_timeout_seconds: settings.generation_timeout_seconds, click_retries: settings.click_retries, new_chat_every: settings.new_chat_every, visual_threshold: selectedProfile?.visual_threshold ?? 0.82 }; }
async function persistProfile(profile: Profile) { await invoke("save_profile", { value: profile }); selectedProfile = profile; settings.selected_profile_id = profile.id; await saveSettings(); profiles = await invoke<Profile[]>("list_profiles"); }
async function saveProfile() {
  try { const input = document.querySelector("#profile-name") as HTMLInputElement; const name = input.value.trim() || selectedProfile?.name; if (!name) throw new Error("Введите название профиля"); await persistProfile(makeProfile(name, selectedProfile?.id)); render(); } catch (error) { showError(String(error)); }
}
function showError(message: string) { latest = { state: "ERROR", message, processed: latest?.processed ?? 0, elapsed_seconds: latest?.elapsed_seconds ?? 0, error: message, progress: latest?.progress }; addLog(latest); render(); }

async function boot() {
  try { settings = { ...settings, ...(await invoke<Settings>("load_settings")) }; templateDefaults = await invoke<TemplateDefaults>("default_template_paths", { theme: settings.theme }); screens = await invoke<ScreenInfo[]>("list_screens"); selectedScreen = screens.find((item) => item.is_primary) ?? screens[0] ?? null; profiles = await invoke<Profile[]>("list_profiles"); selectedProfile = profiles.find((item) => item.id === settings.selected_profile_id) ?? null; if (selectedProfile) { const hydrated = { ...selectedProfile, zones: selectedProfile.zones.map(withTemplateDefaults) }; selectedProfile = hydrated; zones = structuredClone(hydrated.zones); if (JSON.stringify(hydrated.zones) !== JSON.stringify(profiles.find((item) => item.id === hydrated.id)?.zones)) await invoke("save_profile", { value: hydrated }); } selectedScreen = screens.find((item) => item.index === selectedProfile?.screen_index) ?? selectedScreen; if (!selectedProfile) zones = []; storedProgress = await invoke<ProgressState>("load_progress"); } catch (error) { showError(String(error)); }
  await listen<RunEvent>("runner:event", (event) => { latest = { ...event.payload, progress: event.payload.progress ?? latest?.progress }; addLog(latest); render(); });
  await listen<CalibrationResult>("calibration:zones", async (event) => { try { zones = event.payload.zones; selectedScreen = event.payload.screen; const name = event.payload.profile_name?.trim() || selectedProfile?.name || "PromptRunner · " + selectedScreen.width + "×" + selectedScreen.height; const profile = makeProfile(name, selectedProfile?.id); await persistProfile(profile); latest = { state: "READY", message: "Разметка и профиль сохранены: " + profile.name, processed: latest?.processed ?? 0, elapsed_seconds: latest?.elapsed_seconds ?? 0 }; } catch (error) { showError("Не удалось сохранить профиль: " + String(error)); } render(); });
  render();
  await getCurrentWebviewWindow().show();
  if (settings.bank_root) void validate();
}

let calibrationRenderSerial = 0;
async function renderCalibration(requestedScreenIndex?: number) {
  const renderSerial = ++calibrationRenderSerial;
  const available = await invoke<ScreenInfo[]>("list_screens");
  calibrationDraft = specs.map(([name]) => structuredClone(zone(name)));
  let activeScreen = (requestedScreenIndex === undefined ? undefined : available.find((item) => item.index === requestedScreenIndex)) ?? selectedScreen ?? available.find((item) => item.is_primary) ?? available[0];
  if (!activeScreen) { document.body.textContent = "Не найден доступный экран"; return; }
  document.body.className = "calibration-body";
  let zoom = 0.75;
  const fixedZones = new Set<string>();
  const defaultSize = (name: string, kind: ZoneKind) => ({
    width: kind === "observation" ? 80 : name === "input" ? 320 : 20,
    height: kind === "observation" ? 80 : name === "input" ? 80 : 20,
  });
  const draftItem = (name: string) => calibrationDraft.find((item) => item.name === name);
  const draw = async (capture: ScreenCapture) => {
    const pixelScaleX = capture.image_width / Math.max(1, capture.screen.width);
    const pixelScaleY = capture.image_height / Math.max(1, capture.screen.height);
    const viewScaleX = pixelScaleX * zoom;
    const viewScaleY = pixelScaleY * zoom;
    const items = specs.map(([name, label, purpose, kind]) => {
      const item = draftItem(name) ?? zone(name, calibrationDraft);
      const placed = item.width > 0 && item.height > 0 && item.screen_index === capture.screen.index;
      const size = defaultSize(name, kind);
      return { ...item, x: item.x * viewScaleX, y: item.y * viewScaleY, width: (item.width || size.width) * viewScaleX, height: (item.height || size.height) * viewScaleY, placed, label, purpose, kind };
    });
    const placedItems = items.filter((item) => item.placed);
    const placedCount = placedItems.length;
    const fixedCount = items.filter((item) => item.placed && fixedZones.has(item.name)).length;
    const zoomOptions = [[0.1, "10%"], [0.15, "15%"], [0.2, "20%"], [0.25, "25%"], [0.33, "33%"], [0.5, "50%"], [0.75, "75%"], [1, "100%"], [1.5, "150%"], [2, "200%"], [3, "300%"], [4, "400%"], [5, "500%"]] as const;
    const handles = ["n", "ne", "e", "se", "s", "sw", "w", "nw"];
    const palette = items.map((item) => { const fixed = fixedZones.has(item.name); return '<div class="palette-item ' + item.kind + (item.placed ? ' placed' : '') + (fixed ? ' fixed' : '') + '" data-palette-item="' + item.name + '"><button type="button" draggable="' + (!item.placed && !fixed) + '" class="palette-zone ' + item.kind + '" data-palette="' + item.name + '"><span class="palette-swatch" aria-hidden="true"></span><span>' + item.label + '<small>' + item.name + '</small></span></button><button type="button" class="palette-fix" data-fix="' + item.name + '"' + (item.placed ? '' : ' disabled') + '>' + (fixed ? 'Отменить фиксацию' : 'Зафиксировать') + '</button><span class="palette-status">' + (fixed ? '✓ Зафиксировано' : item.placed ? 'Не зафиксировано' : 'Сначала перетащите') + '</span></div>'; }).join("");
    document.body.innerHTML = '<div class="calibration-toolbar"><strong>PromptRunner · снимок всего экрана</strong><label>Монитор<select id="cal-screen">' + available.map((item) => '<option value="' + item.index + '"' + (item.index === capture.screen.index ? " selected" : "") + '>' + (item.is_primary ? "Основной · " : "Монитор · ") + item.width + "×" + item.height + " · дисплей " + (item.index + 1) + '</option>').join("") + '</select></label><label>Масштаб<select id="cal-zoom">' + zoomOptions.map(([value, label]) => '<option value="' + value + '"' + (Math.abs(value - zoom) < 0.001 ? " selected" : "") + '>' + label + '</option>').join("") + '</select></label><input id="cal-profile-name" placeholder="Имя профиля"><span id="calibration-status" class="calibration-status">Калибровка ещё не сохранена</span><span>Сначала перетащите мини-зону, измените её стороны, затем зафиксируйте зелёной кнопкой.</span><button id="calibration-save" class="primary">Сохранить (' + fixedCount + '/' + specs.length + ' зафиксировано)</button><button id="calibration-cancel">Отмена</button></div><div class="calibration-palette">' + palette + '<span class="palette-hint">✓ зелёный статус — зона зафиксирована. Отмените фиксацию, чтобы снова двигать и менять размер.</span></div><div id="calibration-scroll"><div id="calibration-stage" style="width:' + capture.image_width * zoom + 'px;height:' + capture.image_height * zoom + 'px"><img src="' + capture.data_url + '" width="' + capture.image_width * zoom + '" height="' + capture.image_height * zoom + '" draggable="false" alt="Фон снимка экрана">' + placedItems.map((item) => '<div class="cal-zone ' + item.kind + (fixedZones.has(item.name) ? ' fixed' : '') + '" data-zone="' + item.name + '" aria-label="' + item.label + '" style="left:' + item.x + 'px;top:' + item.y + 'px;width:' + item.width + 'px;height:' + item.height + 'px">' + handles.map((handle) => '<i class="resize-handle handle-' + handle + '" data-handle="' + handle + '" aria-hidden="true"></i>').join('') + '</div>').join("") + "</div></div>";
    document.querySelector<HTMLSelectElement>("#cal-screen")!.addEventListener("change", async (event) => { const index = Number((event.target as HTMLSelectElement).value); await invoke("open_calibration", { screenIndex: index }); });
    document.querySelector<HTMLSelectElement>("#cal-zoom")!.addEventListener("change", async (event) => { zoom = Number((event.target as HTMLSelectElement).value); await draw(capture); });
    document.onkeydown = (event) => { if (!(event.metaKey || event.ctrlKey)) return; if (event.key === "+" || event.key === "=") { event.preventDefault(); zoom = Math.min(5, Math.round((zoom + 0.1) * 100) / 100); void draw(capture); } else if (event.key === "-" || event.key === "_") { event.preventDefault(); zoom = Math.max(0.1, Math.round((zoom - 0.1) * 100) / 100); void draw(capture); } else if (event.key === "0") { event.preventDefault(); zoom = 1; void draw(capture); } };
    document.onwheel = (event) => { if (!(event.metaKey || event.ctrlKey)) return; event.preventDefault(); zoom = Math.max(0.1, Math.min(5, Math.round((zoom + (event.deltaY < 0 ? 0.1 : -0.1)) * 100) / 100)); void draw(capture); };
    let active: HTMLElement | null = null; let offsetX = 0; let offsetY = 0; let resizing = false; let resizeHandle = "";
    const stage = document.querySelector<HTMLElement>("#calibration-stage")!;
    const updateDraft = (box: HTMLElement) => { const item = draftItem(box.dataset.zone ?? ""); if (!item) return; item.x = Math.max(0, Math.round(box.offsetLeft / viewScaleX)); item.y = Math.max(0, Math.round(box.offsetTop / viewScaleY)); item.width = Math.max(1, Math.round(box.offsetWidth / viewScaleX)); item.height = Math.max(1, Math.round(box.offsetHeight / viewScaleY)); item.screen_index = capture.screen.index; };
    const selectPalette = (name: string) => { document.querySelectorAll<HTMLElement>(".cal-zone, .palette-zone").forEach((item) => item.classList.toggle("active", item.dataset.zone === name || item.dataset.palette === name)); };
    const placeZone = (name: string, clientX: number, clientY: number) => { const definition = specs.find((item) => item[0] === name); if (!definition || fixedZones.has(name)) return; const item = draftItem(name) ?? zone(name, calibrationDraft); if (item.width > 0 && item.height > 0 && item.screen_index === capture.screen.index) return; const size = defaultSize(name, definition[3]); const rect = stage.getBoundingClientRect(); item.x = Math.max(0, Math.round((clientX - rect.left - size.width * viewScaleX / 2) / viewScaleX)); item.y = Math.max(0, Math.round((clientY - rect.top - size.height * viewScaleY / 2) / viewScaleY)); item.width = size.width; item.height = size.height; item.screen_index = capture.screen.index; if (!draftItem(name)) calibrationDraft.push(item); void draw(capture).then(() => selectPalette(name)); };
    document.querySelectorAll<HTMLButtonElement>("[data-palette]").forEach((button) => { button.addEventListener("click", () => selectPalette(button.dataset.palette ?? "")); button.addEventListener("dragstart", (event) => { const name = button.dataset.palette ?? ""; if (fixedZones.has(name) || draftItem(name)?.width) { event.preventDefault(); return; } event.dataTransfer?.setData("text/plain", name); if (event.dataTransfer) event.dataTransfer.effectAllowed = "copy"; selectPalette(name); }); });
    document.querySelectorAll<HTMLButtonElement>("[data-fix]").forEach((button) => button.addEventListener("click", async () => { const name = button.dataset.fix ?? ""; const item = draftItem(name); if (!item || item.width < 1 || item.height < 1 || item.screen_index !== capture.screen.index) return; if (fixedZones.has(name)) fixedZones.delete(name); else fixedZones.add(name); await draw(capture); if (fixedZones.has(name)) { const next = specs.find(([candidate]) => !fixedZones.has(candidate) && !(draftItem(candidate)?.width && draftItem(candidate)?.height)); selectPalette(next?.[0] ?? name); } else selectPalette(name); }));
    stage.addEventListener("dragover", (event) => { event.preventDefault(); if (event.dataTransfer) event.dataTransfer.dropEffect = "copy"; });
    stage.addEventListener("drop", (event) => { event.preventDefault(); const name = event.dataTransfer?.getData("text/plain"); if (name) placeZone(name, event.clientX, event.clientY); });
    stage.addEventListener("click", (event) => { const name = document.querySelector<HTMLElement>(".palette-zone.active")?.dataset.palette; if (name && event.target === stage) placeZone(name, event.clientX, event.clientY); });
    document.querySelectorAll<HTMLElement>(".cal-zone").forEach((box) => box.addEventListener("pointerdown", (event) => { event.preventDefault(); if (fixedZones.has(box.dataset.zone ?? "")) return; active = box; const handle = (event.target as HTMLElement).closest<HTMLElement>("[data-handle]"); resizing = Boolean(handle); resizeHandle = handle?.dataset.handle ?? ""; const rect = stage.getBoundingClientRect(); offsetX = event.clientX - rect.left - box.offsetLeft; offsetY = event.clientY - rect.top - box.offsetTop; box.classList.add("active"); box.setPointerCapture(event.pointerId); }));
    document.querySelectorAll<HTMLElement>(".cal-zone").forEach((box) => box.addEventListener("pointermove", (event) => { if (active !== box) return; const rect = stage.getBoundingClientRect(); const localX = event.clientX - rect.left; const localY = event.clientY - rect.top; if (resizing) { let left = box.offsetLeft; let top = box.offsetTop; let right = left + box.offsetWidth; let bottom = top + box.offsetHeight; if (resizeHandle.includes("w")) left = Math.max(0, Math.min(localX, right - 1)); if (resizeHandle.includes("e")) right = Math.min(stage.clientWidth, Math.max(localX, left + 1)); if (resizeHandle.includes("n")) top = Math.max(0, Math.min(localY, bottom - 1)); if (resizeHandle.includes("s")) bottom = Math.min(stage.clientHeight, Math.max(localY, top + 1)); box.style.left = left + "px"; box.style.top = top + "px"; box.style.width = Math.max(1, right - left) + "px"; box.style.height = Math.max(1, bottom - top) + "px"; } else { box.style.left = Math.max(0, Math.min(stage.clientWidth - box.offsetWidth, localX - offsetX)) + "px"; box.style.top = Math.max(0, Math.min(stage.clientHeight - box.offsetHeight, localY - offsetY)) + "px"; } updateDraft(box); }));
    document.querySelectorAll<HTMLElement>(".cal-zone").forEach((box) => { box.addEventListener("pointerup", () => { active = null; }); box.addEventListener("pointercancel", () => { active = null; }); });
    document.querySelector("#calibration-save")!.addEventListener("click", async () => { try { const result = specs.map(([name, label, purpose, kind]) => { const item = draftItem(name); if (!item || item.width < 1 || item.height < 1 || item.screen_index !== capture.screen.index || !fixedZones.has(name)) throw new Error("Разместите и зафиксируйте зону «" + label + "»"); return { ...item, name, kind, purpose, screen_index: capture.screen.index }; }); const profileName = (document.querySelector("#cal-profile-name") as HTMLInputElement).value.trim(); await emitTo("main", "calibration:zones", { screen: capture.screen, zones: result, profile_name: profileName || undefined }); await getCurrentWebviewWindow().hide(); } catch (error) { const status = document.querySelector("#calibration-status"); if (status) { status.textContent = String(error); status.className = "calibration-error"; } } });
    document.querySelector("#calibration-cancel")!.addEventListener("click", () => getCurrentWebviewWindow().hide());
  };
  const capture = await invoke<ScreenCapture>("capture_screen", { screenIndex: activeScreen.index });
  if (renderSerial === calibrationRenderSerial) await draw(capture);
}

const currentWindow = getCurrentWebviewWindow();
if ((await currentWindow.label) === "calibration") {
  await listen<number>("calibration:refresh", (event) => renderCalibration(event.payload).catch((error) => { document.body.textContent = "Ошибка обновления разметки: " + String(error); }));
  void renderCalibration().catch((error) => { document.body.textContent = "Ошибка открытия разметки: " + String(error); });
} else void boot();
