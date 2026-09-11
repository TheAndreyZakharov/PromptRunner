use crate::bank;
use crate::error::{AppError, AppResult};
use crate::model::{PromptRequest, Question};
use std::fs;
use std::path::Path;

pub const PROMPT_FILE: &str = "tools/answer-generation-prompt.md";

pub fn build(request: &PromptRequest) -> AppResult<String> {
    let root = Path::new(&request.bank_root);
    let questions = bank::load_questions(root, &request.active_language)?;
    let context_questions = bank::context_questions(root, &request.active_language)?;
    let current = questions
        .iter()
        .find(|question| question.id == request.question_id)
        .ok_or_else(|| AppError::message(format!("Вопрос {} не найден", request.question_id)))?;
    let template_path = root.join(PROMPT_FILE);
    let template = fs::read_to_string(&template_path).map_err(|_| {
        AppError::message(format!(
            "Не найден универсальный промпт: {}",
            template_path.display()
        ))
    })?;
    Ok(render(&template, &context_questions, current))
}

pub fn render(template: &str, questions: &[Question], current: &Question) -> String {
    let context = bank::context_for_subtopic(questions, current);
    format!(
        "{}\n\n## Контекст текущей подтемы\n{}\n\n## Текущий вопрос\n{} [id: {}]",
        template.trim_end(),
        context,
        current.text,
        current.id
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prompt_contains_context_and_current_question() {
        let q = Question {
            id: "RU-1".into(),
            language: "RU".into(),
            text: "Что такое тест?".into(),
            section: "1".into(),
            subtopic: "1".into(),
            source_file: "q.md".into(),
            answer_file: "a.md".into(),
            source_line: 1,
        };
        let prompt = render("Base", std::slice::from_ref(&q), &q);
        assert!(prompt.contains("Что такое тест?"));
        assert!(prompt.contains("[id: RU-1]"));
    }
}
