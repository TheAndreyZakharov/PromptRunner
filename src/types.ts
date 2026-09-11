export type ZoneKind = "action" | "observation";

export interface Zone {
  name: string;
  kind: ZoneKind;
  purpose: string;
  x: number;
  y: number;
  width: number;
  height: number;
  screen_index: number;
  sample_busy?: string;
  sample_ready?: string;
  sample_empty?: string;
}

export interface Profile {
  id: string;
  name: string;
  platform: string;
  screen_index: number;
  screen_width: number;
  screen_height: number;
  scale_factor: number;
  screen_id: number;
  screen_x: number;
  screen_y: number;
  zones: Zone[];
  poll_interval_seconds: number;
  generation_timeout_seconds: number;
  click_retries: number;
  new_chat_every: number;
  visual_threshold?: number;
}

export interface Settings {
  bank_root: string;
  allowed_languages: string[];
  active_language: string;
  selected_profile_id?: string;
  poll_interval_seconds: number;
  generation_timeout_seconds: number;
  click_retries: number;
  new_chat_every: number;
  theme: "system" | "light" | "dark";
  min_pause_seconds: number;
  ready_confirmations: number;
  preserve_clipboard: boolean;
}

export interface Summary {
  root: string;
  total_questions: number;
  unanswered_questions: number;
  answered_questions: number;
  next_question_id?: string;
  languages: Record<string, number>;
  sections: Record<string, number>;
  progress: BankProgress;
  current_section?: SectionProgress;
  current_subtopic?: SubtopicProgress;
  warnings: string[];
}

export interface ProgressMetric {
  completed: number;
  total: number;
  percent: number;
}

export interface SubtopicProgress {
  name: string;
  progress: ProgressMetric;
}

export interface SectionProgress {
  name: string;
  progress: ProgressMetric;
  subtopics: SubtopicProgress[];
}

export interface BankProgress {
  overall: ProgressMetric;
  sections: SectionProgress[];
}

export interface SessionProgress {
  processed: number;
  total?: number;
  percent?: number;
}

export interface ProgressSnapshot {
  bank: BankProgress;
  current_section?: SectionProgress;
  current_subtopic?: SubtopicProgress;
  session: SessionProgress;
}

export interface RunEvent {
  state: string;
  message: string;
  question_id?: string;
  processed: number;
  session_limit?: number;
  started_at?: string;
  elapsed_seconds: number;
  error?: string;
  progress?: ProgressSnapshot;
}

export interface ScreenInfo {
  index: number;
  id: number;
  x: number;
  y: number;
  width: number;
  height: number;
  scale_factor: number;
  is_primary: boolean;
}

export interface ScreenCapture {
  screen: ScreenInfo;
  data_url: string;
  image_width: number;
  image_height: number;
}

export interface TemplateDefaults {
  sample_busy?: string;
  sample_ready?: string;
  sample_empty?: string;
}

export interface CalibrationResult {
  screen: ScreenInfo;
  zones: Zone[];
  profile_name?: string;
}

export interface ProgressState {
  bank_root: string;
  active_language: string;
  session_started_at?: string;
  current_question_id?: string;
  last_successful_id?: string;
  processed: number;
  session_limit?: number;
  status: string;
  updated_at?: string;
}
