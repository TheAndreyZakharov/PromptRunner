use crate::error::{AppError, AppResult};
use crate::model::{
    BankProgress, BankSummary, ProgressMetric, ProgressSnapshot, Question, SectionProgress,
    SessionProgress, SubtopicProgress,
};
use regex::Regex;
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
struct ParsedQuestion {
    id: String,
    language: String,
    text: String,
    section: String,
    subtopic: String,
    file: PathBuf,
    line: usize,
}

pub fn normalize_language(value: &str) -> Option<String> {
    match value.trim().to_ascii_uppercase().as_str() {
        "RU" | "RUS" | "RUSSIAN" => Some("RU".to_string()),
        "EN" | "ENG" | "ENGLISH" => Some("EN".to_string()),
        _ => None,
    }
}

pub fn load_questions(root: &Path, language: &str) -> AppResult<Vec<Question>> {
    let language = normalize_language(language)
        .ok_or_else(|| AppError::message("Активный язык должен быть RU или EN"))?;
    let root = resolve_bank_root(root, &language)?;
    load_questions_from_directory(&root, &language, true)
}

pub fn context_questions(root: &Path, language: &str) -> AppResult<Vec<Question>> {
    let language = normalize_language(language)
        .ok_or_else(|| AppError::message("Активный язык должен быть RU или EN"))?;
    let root = resolve_bank_root(root, &language)?;
    load_questions_from_directory(&root, &language, false)
}

fn resolve_bank_root(root: &Path, language: &str) -> AppResult<PathBuf> {
    let mut candidates = Vec::new();
    let mut candidate = Some(root.to_path_buf());
    for _ in 0..3 {
        if let Some(path) = candidate {
            candidates.push(path.clone());
            candidate = path.parent().map(Path::to_path_buf);
        }
    }
    let answer_name = format!("Questions_with_AI_Answers_By_Topic_{language}");
    let source_name = format!("Questions_By_Topic_{language}");
    candidates
        .into_iter()
        .find(|candidate| {
            candidate.join("tools/answer-generation-prompt.md").is_file()
                && candidate.join(language).join(&answer_name).is_dir()
                && candidate.join(language).join(&source_name).is_dir()
        })
        .ok_or_else(|| {
            AppError::message(format!(
                "Не найден корень банка для {language}. Выберите папку репозитория IT-Interview-Question-Bank"
            ))
        })
}

fn load_questions_from_directory(
    root: &Path,
    language: &str,
    from_answers: bool,
) -> AppResult<Vec<Question>> {
    let directory_name = if from_answers {
        format!("Questions_with_AI_Answers_By_Topic_{language}")
    } else {
        format!("Questions_By_Topic_{language}")
    };
    let directory = root.join(language).join(&directory_name);
    if !directory.is_dir() {
        return Err(AppError::message(format!(
            "Не найден каталог {} для {}: {}",
            directory_name,
            language,
            directory.display()
        )));
    }
    let mut parsed = Vec::new();
    for entry in WalkDir::new(&directory).into_iter().filter_map(Result::ok) {
        let path = entry.path();
        if !entry.file_type().is_file() || path.extension().and_then(|v| v.to_str()) != Some("md") {
            continue;
        }
        let content = fs::read_to_string(path)?;
        parsed.extend(parse_file(path, &content, language)?);
    }
    parsed.sort_by(|a, b| {
        numeric_key(&a.section)
            .cmp(&numeric_key(&b.section))
            .then(numeric_key(&a.subtopic).cmp(&numeric_key(&b.subtopic)))
            .then(
                numeric_key(&a.id)
                    .cmp(&numeric_key(&b.id))
                    .then(a.id.cmp(&b.id)),
            )
    });
    let mut ids = HashSet::new();
    let mut result = Vec::new();
    for item in parsed {
        if !ids.insert(item.id.clone()) {
            return Err(AppError::message(format!(
                "Обнаружен дубликат ID вопроса: {}",
                item.id
            )));
        }
        let answer_file = if from_answers {
            item.file.clone()
        } else {
            answer_file_for_source(root, &item)
        };
        result.push(Question {
            id: item.id,
            language: item.language,
            text: item.text,
            section: item.section,
            subtopic: item.subtopic,
            source_file: item.file,
            answer_file,
            source_line: item.line,
        });
    }
    Ok(result)
}

pub fn summary(root: &Path, language: &str) -> AppResult<BankSummary> {
    let normalized = normalize_language(language)
        .ok_or_else(|| AppError::message("Активный язык должен быть RU или EN"))?;
    let root = resolve_bank_root(root, &normalized)?;
    let questions = load_questions(&root, &normalized)?;
    let prompt_path = root.join("tools/answer-generation-prompt.md");
    if !prompt_path.is_file() {
        return Err(AppError::message(format!(
            "Не найден универсальный промпт: {}",
            prompt_path.display()
        )));
    }
    if questions.is_empty() {
        return Err(AppError::message(
            "В выбранном каталоге не найдено вопросов с корректными ID",
        ));
    }
    let mut languages = BTreeMap::new();
    let mut sections = BTreeMap::new();
    let mut answered = 0;
    let mut next_question_id = None;
    let answered_ids = answered_question_ids(&questions)?;
    for question in &questions {
        *languages.entry(question.language.clone()).or_insert(0) += 1;
        *sections.entry(question.section.clone()).or_insert(0) += 1;
        if answered_ids.contains(&question.id) {
            answered += 1;
        } else if next_question_id.is_none() {
            next_question_id = Some(question.id.clone());
        }
    }
    let progress = progress(&questions, &answered_ids);
    let current_section = next_question_id.as_ref().and_then(|id| {
        questions
            .iter()
            .find(|question| &question.id == id)
            .and_then(|question| {
                progress
                    .sections
                    .iter()
                    .find(|section| section.name == question.section)
                    .cloned()
            })
    });
    let current_subtopic = next_question_id.as_ref().and_then(|id| {
        questions
            .iter()
            .find(|question| &question.id == id)
            .and_then(|question| {
                current_section
                    .as_ref()?
                    .subtopics
                    .iter()
                    .find(|subtopic| subtopic.name == question.subtopic)
                    .cloned()
            })
    });
    Ok(BankSummary {
        root: root.display().to_string(),
        total_questions: questions.len(),
        unanswered_questions: questions.len().saturating_sub(answered),
        answered_questions: answered,
        next_question_id,
        languages,
        sections,
        progress,
        current_section,
        current_subtopic,
        warnings: Vec::new(),
    })
}

pub fn answered_question_ids(questions: &[Question]) -> AppResult<HashSet<String>> {
    let id_re = Regex::new(r"(?i)\[id:\s*([A-Z]{2}(?:[-/][A-Z0-9]+)+)\]")?;
    let files = questions
        .iter()
        .map(|question| question.answer_file.clone())
        .collect::<HashSet<_>>();
    let mut answered = HashSet::new();
    for path in files {
        if !path.is_file() {
            continue;
        }
        for line in fs::read_to_string(path)?.lines() {
            if !line.trim_start().starts_with("- **") {
                continue;
            }
            if let Some(caps) = id_re.captures(line) {
                if let Some(id) = caps.get(1) {
                    answered.insert(id.as_str().to_ascii_uppercase());
                }
            }
        }
    }
    Ok(answered)
}

pub fn progress(questions: &[Question], answered_ids: &HashSet<String>) -> BankProgress {
    let mut groups: BTreeMap<String, BTreeMap<String, (usize, usize)>> = BTreeMap::new();
    for question in questions {
        let section = groups.entry(question.section.clone()).or_default();
        let counts = section.entry(question.subtopic.clone()).or_default();
        counts.1 += 1;
        if answered_ids.contains(&question.id) {
            counts.0 += 1;
        }
    }

    let mut section_names = groups.keys().cloned().collect::<Vec<_>>();
    section_names.sort_by(|a, b| numeric_key(a).cmp(&numeric_key(b)).then(a.cmp(b)));
    let mut sections = Vec::with_capacity(section_names.len());
    for section_name in section_names {
        let subtopics = groups.remove(&section_name).unwrap_or_default();
        let mut subtopic_names = subtopics.keys().cloned().collect::<Vec<_>>();
        subtopic_names.sort_by(|a, b| numeric_key(a).cmp(&numeric_key(b)).then(a.cmp(b)));
        let mut section_completed = 0;
        let mut section_total = 0;
        let mut progress_subtopics = Vec::with_capacity(subtopic_names.len());
        for subtopic_name in subtopic_names {
            let (completed, total) = subtopics[&subtopic_name];
            section_completed += completed;
            section_total += total;
            progress_subtopics.push(SubtopicProgress {
                name: subtopic_name,
                progress: metric(completed, total),
            });
        }
        sections.push(SectionProgress {
            name: section_name,
            progress: metric(section_completed, section_total),
            subtopics: progress_subtopics,
        });
    }
    let completed = sections
        .iter()
        .map(|section| section.progress.completed)
        .sum();
    let total = sections.iter().map(|section| section.progress.total).sum();
    BankProgress {
        overall: metric(completed, total),
        sections,
    }
}

pub fn progress_snapshot(
    questions: &[Question],
    answered_ids: &HashSet<String>,
    current: Option<&Question>,
    processed: u32,
    session_limit: Option<u32>,
) -> ProgressSnapshot {
    let bank = progress(questions, answered_ids);
    let current_section = current.and_then(|question| {
        bank.sections
            .iter()
            .find(|section| section.name == question.section)
            .cloned()
    });
    let current_subtopic = current_section.as_ref().and_then(|section| {
        current.and_then(|question| {
            section
                .subtopics
                .iter()
                .find(|subtopic| subtopic.name == question.subtopic)
                .cloned()
        })
    });
    ProgressSnapshot {
        bank,
        current_section,
        current_subtopic,
        session: SessionProgress {
            processed,
            total: session_limit,
            percent: session_limit.map(|limit| {
                if limit == 0 {
                    100.0
                } else {
                    (((processed as f32 / limit as f32) * 1000.0).round() / 10.0).min(100.0)
                }
            }),
        },
    }
}

fn metric(completed: usize, total: usize) -> ProgressMetric {
    ProgressMetric {
        completed,
        total,
        percent: if total == 0 {
            0.0
        } else {
            ((completed as f32 / total as f32) * 1000.0).round() / 10.0
        },
    }
}

pub fn fingerprint(root: &Path, language: &str) -> AppResult<String> {
    let language = normalize_language(language)
        .ok_or_else(|| AppError::message("Активный язык должен быть RU или EN"))?;
    let root = resolve_bank_root(root, &language)?;
    let source_dir = root
        .join(&language)
        .join(format!("Questions_with_AI_Answers_By_Topic_{language}"));
    if !source_dir.is_dir() {
        return Err(AppError::message(format!(
            "Не найден каталог вопросов для {language}: {}",
            source_dir.display()
        )));
    }
    let mut files = WalkDir::new(&source_dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .map(|entry| entry.into_path())
        .collect::<Vec<_>>();
    files.sort();
    files.push(root.join("tools/answer-generation-prompt.md"));
    let mut hasher = Sha256::new();
    for path in files {
        hasher.update(
            path.strip_prefix(&root)
                .unwrap_or(&path)
                .to_string_lossy()
                .as_bytes(),
        );
        hasher.update([0]);
        hasher.update(fs::read(path)?);
        hasher.update([0]);
    }
    Ok(format!("sha256:{:x}", hasher.finalize()))
}

pub fn next_question(
    questions: &[Question],
    start_id: Option<&str>,
) -> AppResult<Option<Question>> {
    for question in questions {
        if start_id
            .map(|id| numeric_key(&question.id) < numeric_key(id))
            .unwrap_or(false)
        {
            continue;
        }
        if !is_answered(&question.answer_file, &question.id)? {
            return Ok(Some(question.clone()));
        }
    }
    Ok(None)
}

/// Select the first unanswered question strictly after the question that was
/// just processed. Using the in-memory answered set and a strict boundary
/// prevents a stale answer-file read from selecting the same ID again.
pub fn next_question_after(
    questions: &[Question],
    answered_ids: &HashSet<String>,
    current_id: &str,
) -> Option<Question> {
    let Some(position) = questions
        .iter()
        .position(|question| question.id == current_id)
    else {
        return None;
    };
    questions
        .iter()
        .skip(position + 1)
        .find(|question| !answered_ids.contains(&question.id))
        .cloned()
}

pub fn context_for_subtopic(questions: &[Question], current: &Question) -> String {
    questions
        .iter()
        .filter(|question| {
            question.section == current.section && question.subtopic == current.subtopic
        })
        .map(|question| format!("- {} [id: {}]", question.text, question.id))
        .collect::<Vec<_>>()
        .join("\n")
}

fn parse_file(path: &Path, content: &str, language: &str) -> AppResult<Vec<ParsedQuestion>> {
    let id_re = Regex::new(r"(?i)\[id:\s*([A-Z]{2}(?:[-/][A-Z0-9]+)+)\]")?;
    let heading_re = Regex::new(r"^(#{1,6})\s+(.+?)\s*$")?;
    let mut section = String::from("0");
    let mut subtopic = String::from("0");
    let mut headings: Vec<(usize, String)> = Vec::new();
    let mut result = Vec::new();
    for (index, line) in content.lines().enumerate() {
        if let Some(caps) = heading_re.captures(line) {
            let title = caps.get(2).map(|m| m.as_str().trim()).unwrap_or_default();
            let number = title
                .split_whitespace()
                .next()
                .unwrap_or("0")
                .trim_end_matches('.');
            let level = caps.get(1).map(|m| m.as_str().len()).unwrap_or(1);
            while headings
                .last()
                .map(|(previous, _)| *previous >= level)
                .unwrap_or(false)
            {
                headings.pop();
            }
            headings.push((level, title.to_string()));
            if level <= 2 {
                section = number.to_string();
            }
            subtopic = number.to_string();
        }
        if !line.trim_start().starts_with('-') {
            continue;
        }
        let Some(caps) = id_re.captures(line) else {
            continue;
        };
        let id = caps
            .get(1)
            .map(|m| m.as_str().to_ascii_uppercase())
            .unwrap_or_default();
        if !id.starts_with(language) {
            continue;
        }
        let text = line
            .replace(caps.get(0).map(|m| m.as_str()).unwrap_or_default(), "")
            .trim()
            .trim_start_matches(['-', '*', ' '])
            .trim()
            .to_string();
        if text.is_empty() {
            continue;
        }
        result.push(ParsedQuestion {
            id,
            language: language.to_string(),
            text,
            section: section.clone(),
            subtopic: subtopic.clone(),
            file: path.to_path_buf(),
            line: index + 1,
        });
    }
    Ok(result)
}

fn answer_file_for_source(root: &Path, question: &ParsedQuestion) -> PathBuf {
    let source_dir = root
        .join(&question.language)
        .join(format!("Questions_By_Topic_{}", question.language));
    let answer_dir = root.join(&question.language).join(format!(
        "Questions_with_AI_Answers_By_Topic_{}",
        question.language
    ));
    question
        .file
        .strip_prefix(source_dir)
        .map(|relative| answer_dir.join(relative))
        .unwrap_or_else(|_| answer_dir.join(question.file.file_name().unwrap_or_default()))
}

pub fn is_answered(path: &Path, id: &str) -> AppResult<bool> {
    if !path.exists() {
        return Ok(false);
    }
    let content = fs::read_to_string(path)?;
    Ok(content.lines().any(|line| {
        line.contains(&format!("[id: {}]", id)) && line.trim_start().starts_with("- **")
    }))
}

pub fn numeric_key(id: &str) -> Vec<u64> {
    id.split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn language_is_normalized() {
        assert_eq!(normalize_language("english"), Some("EN".into()));
    }
    #[test]
    fn numeric_ids_sort_naturally() {
        assert!(numeric_key("RU-2").cmp(&numeric_key("RU-10")).is_lt());
    }

    #[test]
    fn next_question_after_is_strictly_after_current() {
        let question = |id: &str| Question {
            id: id.into(),
            language: "RU".into(),
            text: id.into(),
            section: "1".into(),
            subtopic: "1".into(),
            source_file: "questions.md".into(),
            answer_file: id.into(),
            source_line: 1,
        };
        let questions = vec![
            question("RU-004901"),
            question("RU-004902"),
            question("RU-004903"),
        ];
        let mut answered = HashSet::new();
        answered.insert("RU-004902".into());
        assert_eq!(
            next_question_after(&questions, &answered, "RU-004902")
                .unwrap()
                .id,
            "RU-004903"
        );
        assert!(next_question_after(&questions, &answered, "RU-004903").is_none());
    }
    #[test]
    fn progress_groups_questions_by_section_and_subtopic() {
        let question = |id: &str, section: &str, subtopic: &str| Question {
            id: id.into(),
            language: "RU".into(),
            text: id.into(),
            section: section.into(),
            subtopic: subtopic.into(),
            source_file: "questions.md".into(),
            answer_file: "answers.md".into(),
            source_line: 1,
        };
        let questions = vec![
            question("RU-1", "1", "1.1"),
            question("RU-2", "1", "1.1"),
            question("RU-3", "1", "1.2"),
            question("RU-4", "2", "2.1"),
        ];
        let answered = ["RU-1", "RU-4"]
            .into_iter()
            .map(str::to_string)
            .collect::<HashSet<_>>();
        let result = progress_snapshot(&questions, &answered, Some(&questions[1]), 3, Some(10));
        assert_eq!(result.bank.overall.completed, 2);
        assert_eq!(result.bank.overall.total, 4);
        assert_eq!(result.bank.sections[0].progress.completed, 1);
        assert_eq!(result.bank.sections[0].progress.total, 3);
        assert_eq!(result.bank.sections[0].subtopics.len(), 2);
        assert_eq!(result.current_section.as_ref().unwrap().name, "1");
        assert_eq!(result.current_subtopic.as_ref().unwrap().name, "1.1");
        assert_eq!(result.session.percent, Some(30.0));
    }
    #[test]
    fn real_bank_smoke_test_when_configured() {
        let Ok(root) = std::env::var("PROMPTRUNNER_BANK_ROOT") else {
            return;
        };
        let questions = load_questions(Path::new(&root), "RU").expect("real RU bank should parse");
        let questions_from_language_dir = load_questions(&Path::new(&root).join("RU"), "RU")
            .expect("RU language directory should resolve to the bank root");
        let context = context_questions(Path::new(&root), "RU")
            .expect("source question directory should parse");
        assert!(!questions.is_empty());
        assert_eq!(questions.len(), questions_from_language_dir.len());
        assert!(!context.is_empty());
        assert!(context.iter().any(|question| question.id == "RU-000001"));
        assert!(questions
            .iter()
            .all(|question| question.id.starts_with("RU-")));
        assert!(questions.iter().any(|question| question.id == "RU-000001"));
        assert!(questions.iter().any(|question| question
            .answer_file
            .to_string_lossy()
            .contains("Questions_with_AI_Answers_By_Topic_RU")));
        let first = questions
            .iter()
            .find(|question| question.id == "RU-000001")
            .unwrap();
        assert!(
            first.answer_file.exists(),
            "answer mapping should point to an existing answer file: {}",
            first.answer_file.display()
        );
        let bank_summary = summary(Path::new(&root), "RU").expect("real RU summary should parse");
        assert!(bank_summary.next_question_id.is_some());
    }
}
