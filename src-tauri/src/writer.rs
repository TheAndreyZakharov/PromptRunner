use crate::error::{AppError, AppResult};
use crate::model::Question;
use std::fs;
use std::time::{SystemTime, UNIX_EPOCH};

pub fn format_answer(question: &Question, clipboard_text: &str) -> AppResult<String> {
    let text = clipboard_text.trim();
    if text.is_empty() {
        return Err(AppError::message("Буфер обмена пуст"));
    }
    if !text.contains(&question.id) {
        return Err(AppError::message(format!(
            "Буфер не содержит ID {}",
            question.id
        )));
    }
    let first_line = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or_default();
    let expected_marker = format!("[id: {}]", question.id);
    if !first_line.contains(&expected_marker) || !first_line.trim_start().starts_with("- **") {
        return Err(AppError::message(format!(
            "Первая строка clipboard не соответствует вопросу {}",
            question.id
        )));
    }
    let lower = text.to_lowercase();
    if !lower.contains("*ответ:*") && !lower.contains("*answer:*") {
        return Err(AppError::message("Буфер не содержит маркер ответа"));
    }
    Ok(format!("{}\n\n", text))
}

pub fn replace_question(question: &Question, clipboard_text: &str) -> AppResult<()> {
    let rendered = format_answer(question, clipboard_text)?;
    let path = &question.answer_file;
    let parent = path
        .parent()
        .ok_or_else(|| AppError::message("У файла ответа нет каталога"))?;
    fs::create_dir_all(parent)?;
    let original = if path.exists() {
        fs::read_to_string(path)?
    } else {
        format!("- {} [id: {}]\n", question.text, question.id)
    };
    let marker = format!("[id: {}]", question.id);
    if original
        .lines()
        .any(|line| line.contains(&marker) && line.contains("**"))
    {
        return Err(AppError::message(format!(
            "Вопрос {} уже содержит готовый ответ",
            question.id
        )));
    }
    let mut replaced = false;
    let mut output = String::new();
    for line in original.lines() {
        if !replaced && line.contains(&marker) && !line.starts_with('#') && !line.contains("**") {
            output.push_str(&rendered);
            replaced = true;
        } else {
            output.push_str(line);
            output.push('\n');
        }
    }
    if !replaced {
        return Err(AppError::message(format!(
            "Строка вопроса {} не найдена в {}",
            question.id,
            path.display()
        )));
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap_or_default().to_string_lossy(),
        stamp
    ));
    fs::write(&temp, output)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(&temp, path)?;
    let saved = fs::read_to_string(path)?;
    let saved_lower = saved.to_lowercase();
    if !saved.contains(&marker)
        || (!saved_lower.contains("*ответ:*") && !saved_lower.contains("*answer:*"))
    {
        return Err(AppError::message(format!(
            "Проверка записи ответа {} не пройдена",
            question.id
        )));
    }
    Ok(())
}

pub fn answer_text_from_clipboard() -> AppResult<String> {
    let mut clipboard =
        arboard::Clipboard::new().map_err(|e| AppError::Clipboard(e.to_string()))?;
    clipboard
        .get_text()
        .map_err(|e| AppError::Clipboard(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn answer_requires_question_id() {
        let q = Question {
            id: "RU-7".into(),
            language: "RU".into(),
            text: "Q".into(),
            section: "1".into(),
            subtopic: "1".into(),
            source_file: "q.md".into(),
            answer_file: "a.md".into(),
            source_line: 1,
        };
        assert!(format_answer(&q, "answer").is_err());
        assert_eq!(
            format_answer(&q, "- **Q** [id: RU-7]\n*Ответ:*\nA").unwrap(),
            "- **Q** [id: RU-7]\n*Ответ:*\nA\n\n"
        );
    }

    #[test]
    fn replaces_question_with_answer_block_and_preserves_separator() {
        let root = std::env::temp_dir().join(format!(
            "promptrunner-writer-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let answer_file = root.join("answers.md");
        fs::write(
            &answer_file,
            "- Как работает тест? [id: RU-000001]\n- Следующий вопрос [id: RU-000002]\n",
        )
        .unwrap();
        let question = Question {
            id: "RU-000001".into(),
            language: "RU".into(),
            text: "Как работает тест?".into(),
            section: "1".into(),
            subtopic: "1.1".into(),
            source_file: root.join("questions.md"),
            answer_file: answer_file.clone(),
            source_line: 1,
        };
        replace_question(
            &question,
            "- **Как работает тест?** [id: RU-000001]\n*Ответ:*\n\nПроверяемый ответ.",
        )
        .unwrap();
        let saved = fs::read_to_string(&answer_file).unwrap();
        assert!(saved.contains("*Ответ:*\n\nПроверяемый ответ.\n\n- Следующий вопрос"));
        assert!(replace_question(
            &question,
            "- **Как работает тест?** [id: RU-000001]\n*Ответ:*\n\nДубль"
        )
        .is_err());
        fs::remove_dir_all(root).unwrap();
    }
}
