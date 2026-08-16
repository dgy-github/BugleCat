//! User-question tool and pluggable UI boundary.

use std::rc::Rc;

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::tools::{Tool, ToolContext};

const MAX_QUESTION_CHARS: usize = 2_000;
const MAX_OPTIONS: usize = 8;
const MAX_OPTION_CHARS: usize = 200;

/// A question that requires an explicit answer from the user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserQuestionRequest {
    pub question: String,
    pub options: Vec<String>,
    pub allow_free_text: bool,
}

/// Frontends implement this boundary to present a question and await an answer.
#[async_trait(?Send)]
pub trait UserQuestionHandler {
    async fn request(&self, request: UserQuestionRequest) -> Option<String>;
}

pub(crate) struct AskUserQuestionTool {
    handler: Rc<dyn UserQuestionHandler>,
}

impl AskUserQuestionTool {
    pub(crate) fn new(handler: Rc<dyn UserQuestionHandler>) -> Self {
        Self { handler }
    }
}

#[async_trait(?Send)]
impl Tool for AskUserQuestionTool {
    fn name(&self) -> &str {
        "ask_user_question"
    }

    fn description(&self) -> &str {
        "Ask the user one blocking question when required information cannot be discovered. Prefer concrete options; allow free text only when necessary."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "question": {"type": "string", "description": "A concise question for the user."},
                "options": {
                    "type": "array",
                    "items": {"type": "string"},
                    "maxItems": MAX_OPTIONS,
                    "description": "Optional mutually exclusive answer choices."
                },
                "allow_free_text": {
                    "type": "boolean",
                    "default": true,
                    "description": "Whether the user may enter an answer outside the choices."
                }
            },
            "required": ["question"],
            "additionalProperties": false
        })
    }

    async fn execute(&self, _ctx: &ToolContext, args: &Value) -> String {
        let request = match parse_request(args) {
            Ok(request) => request,
            Err(error) => return json!({"status": "error", "error": error}).to_string(),
        };
        if let Err(error) = validate_request(&request) {
            return json!({"status": "error", "error": error}).to_string();
        }
        match self.handler.request(request).await {
            Some(answer) => json!({"status": "answered", "answer": answer}).to_string(),
            None => json!({"status": "cancelled"}).to_string(),
        }
    }
}

fn parse_request(args: &Value) -> Result<UserQuestionRequest, String> {
    let object = args
        .as_object()
        .ok_or_else(|| "arguments must be an object".to_string())?;
    let question = object
        .get("question")
        .and_then(Value::as_str)
        .ok_or_else(|| "question must be a string".to_string())?
        .to_string();
    let options = match object.get("options") {
        None => Vec::new(),
        Some(Value::Array(values)) => values
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .map(str::to_string)
                    .ok_or_else(|| "each option must be a string".to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        Some(_) => return Err("options must be an array".to_string()),
    };
    let allow_free_text = object
        .get("allow_free_text")
        .map(|value| {
            value
                .as_bool()
                .ok_or_else(|| "allow_free_text must be a boolean".to_string())
        })
        .transpose()?
        .unwrap_or(true);
    Ok(UserQuestionRequest {
        question,
        options,
        allow_free_text,
    })
}

fn validate_request(request: &UserQuestionRequest) -> Result<(), String> {
    let question = request.question.trim();
    if question.is_empty() {
        return Err("question cannot be empty".to_string());
    }
    if question.chars().count() > MAX_QUESTION_CHARS {
        return Err(format!("question exceeds {MAX_QUESTION_CHARS} characters"));
    }
    if request.options.len() > MAX_OPTIONS {
        return Err(format!(
            "options cannot contain more than {MAX_OPTIONS} items"
        ));
    }
    if request.options.iter().any(|option| {
        let option = option.trim();
        option.is_empty() || option.chars().count() > MAX_OPTION_CHARS
    }) {
        return Err(format!(
            "each option must contain 1 to {MAX_OPTION_CHARS} characters"
        ));
    }
    if !request.allow_free_text && request.options.is_empty() {
        return Err("options are required when free-text answers are disabled".to_string());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use ncx_sandbox::SandboxPolicy;

    use super::*;
    use crate::tools::ToolRegistry;

    struct AnsweringHandler {
        seen: Rc<RefCell<Vec<UserQuestionRequest>>>,
        answer: Option<String>,
    }

    #[async_trait(?Send)]
    impl UserQuestionHandler for AnsweringHandler {
        async fn request(&self, request: UserQuestionRequest) -> Option<String> {
            self.seen.borrow_mut().push(request);
            self.answer.clone()
        }
    }

    fn context() -> ToolContext {
        ToolContext::new(
            PathBuf::from("."),
            SandboxPolicy::new("workspace-write", "."),
        )
    }

    #[tokio::test]
    async fn returns_the_frontend_answer() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let tool = AskUserQuestionTool::new(Rc::new(AnsweringHandler {
            seen: seen.clone(),
            answer: Some("Rust".to_string()),
        }));
        let result = tool
            .execute(
                &context(),
                &json!({"question": "Which language?", "options": ["Rust", "Python"]}),
            )
            .await;

        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["answer"],
            "Rust"
        );
        assert_eq!(seen.borrow().len(), 1);
    }

    #[tokio::test]
    async fn rejects_a_choice_only_question_without_options() {
        let seen = Rc::new(RefCell::new(Vec::new()));
        let tool = AskUserQuestionTool::new(Rc::new(AnsweringHandler {
            seen: seen.clone(),
            answer: None,
        }));
        let result = tool
            .execute(
                &context(),
                &json!({"question": "Choose", "allow_free_text": false}),
            )
            .await;

        assert_eq!(
            serde_json::from_str::<Value>(&result).unwrap()["status"],
            "error"
        );
        assert!(seen.borrow().is_empty());
    }

    #[test]
    fn registry_exposes_question_tool_only_with_a_handler() {
        assert!(ToolRegistry::new(context())
            .get("ask_user_question")
            .is_none());

        let handler = Rc::new(AnsweringHandler {
            seen: Rc::new(RefCell::new(Vec::new())),
            answer: None,
        });
        let registry = ToolRegistry::new(context().with_user_question_handler(handler));
        assert!(registry.get("ask_user_question").is_some());
    }
}
