use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Question {
    pub id: String,
    pub language: String,
    pub text: String,
    pub section: String,
    pub subtopic: String,
    pub source_file: PathBuf,
    pub answer_file: PathBuf,
    pub source_line: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BankSummary {
    pub root: String,
    pub total_questions: usize,
    pub unanswered_questions: usize,
    pub answered_questions: usize,
    #[serde(default)]
    pub next_question_id: Option<String>,
    pub languages: BTreeMap<String, usize>,
    pub sections: BTreeMap<String, usize>,
    pub progress: BankProgress,
    #[serde(default)]
    pub current_section: Option<SectionProgress>,
    #[serde(default)]
    pub current_subtopic: Option<SubtopicProgress>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressMetric {
    pub completed: usize,
    pub total: usize,
    pub percent: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SubtopicProgress {
    pub name: String,
    pub progress: ProgressMetric,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SectionProgress {
    pub name: String,
    pub progress: ProgressMetric,
    pub subtopics: Vec<SubtopicProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BankProgress {
    pub overall: ProgressMetric,
    pub sections: Vec<SectionProgress>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SessionProgress {
    pub processed: u32,
    pub total: Option<u32>,
    pub percent: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressSnapshot {
    pub bank: BankProgress,
    #[serde(default)]
    pub current_section: Option<SectionProgress>,
    #[serde(default)]
    pub current_subtopic: Option<SubtopicProgress>,
    pub session: SessionProgress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptRequest {
    pub bank_root: String,
    pub active_language: String,
    pub question_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Zone {
    pub name: String,
    pub kind: String,
    pub purpose: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub screen_index: u32,
    pub sample_busy: Option<String>,
    pub sample_ready: Option<String>,
    #[serde(default)]
    pub sample_empty: Option<String>,
}

impl Zone {
    pub fn is_valid(&self) -> bool {
        self.width > 0 && self.height > 0 && self.x >= 0 && self.y >= 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub platform: String,
    pub screen_index: u32,
    pub screen_width: u32,
    pub screen_height: u32,
    pub scale_factor: f32,
    #[serde(default)]
    pub screen_id: u32,
    #[serde(default)]
    pub screen_x: i32,
    #[serde(default)]
    pub screen_y: i32,
    pub zones: Vec<Zone>,
    pub poll_interval_seconds: u64,
    #[serde(default = "default_send_retry_delay_seconds")]
    pub send_retry_delay_seconds: u64,
    pub generation_timeout_seconds: u64,
    pub click_retries: u8,
    pub new_chat_every: u32,
    #[serde(default = "default_scroll_after_seconds")]
    pub scroll_after_seconds: u64,
    #[serde(default = "default_visual_threshold")]
    pub visual_threshold: f32,
}

fn default_visual_threshold() -> f32 {
    0.82
}

fn default_send_retry_delay_seconds() -> u64 {
    10
}

fn default_scroll_after_seconds() -> u64 {
    2
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: String::new(),
            platform: std::env::consts::OS.to_string(),
            screen_index: 0,
            screen_width: 0,
            screen_height: 0,
            scale_factor: 1.0,
            screen_id: 0,
            screen_x: 0,
            screen_y: 0,
            zones: Vec::new(),
            poll_interval_seconds: 10,
            send_retry_delay_seconds: 10,
            generation_timeout_seconds: 600,
            click_retries: 3,
            new_chat_every: 100,
            scroll_after_seconds: 2,
            visual_threshold: 0.82,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    pub bank_root: String,
    pub active_language: String,
    pub allowed_languages: Vec<String>,
    pub start_id: Option<String>,
    pub session_limit: Option<u32>,
    #[serde(default)]
    pub min_pause_seconds: u64,
    #[serde(default = "default_ready_confirmations")]
    pub ready_confirmations: u8,
    #[serde(default)]
    pub preserve_clipboard: bool,
    pub profile: Profile,
    pub dry_run: bool,
}

fn default_ready_confirmations() -> u8 {
    2
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunEvent {
    pub state: String,
    pub message: String,
    pub question_id: Option<String>,
    pub processed: u32,
    pub session_limit: Option<u32>,
    pub started_at: Option<String>,
    pub elapsed_seconds: u64,
    pub error: Option<String>,
    #[serde(default)]
    pub progress: Option<ProgressSnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub bank_root: String,
    pub allowed_languages: Vec<String>,
    pub active_language: String,
    pub selected_profile_id: Option<String>,
    pub poll_interval_seconds: u64,
    #[serde(default = "default_send_retry_delay_seconds")]
    pub send_retry_delay_seconds: u64,
    pub generation_timeout_seconds: u64,
    pub click_retries: u8,
    pub new_chat_every: u32,
    #[serde(default = "default_scroll_after_seconds")]
    pub scroll_after_seconds: u64,
    pub theme: String,
    #[serde(default)]
    pub min_pause_seconds: u64,
    #[serde(default = "default_ready_confirmations")]
    pub ready_confirmations: u8,
    #[serde(default)]
    pub preserve_clipboard: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenInfo {
    pub index: u32,
    pub id: u32,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub is_primary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScreenCapture {
    pub screen: ScreenInfo,
    pub data_url: String,
    pub image_width: u32,
    pub image_height: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TemplateDefaults {
    pub sample_busy: Option<String>,
    pub sample_ready: Option<String>,
    pub sample_empty: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProgressState {
    pub bank_root: String,
    pub active_language: String,
    #[serde(default)]
    pub bank_fingerprint: String,
    pub session_started_at: Option<String>,
    pub current_question_id: Option<String>,
    pub last_successful_id: Option<String>,
    pub processed: u32,
    pub session_limit: Option<u32>,
    pub status: String,
    pub updated_at: Option<String>,
}
