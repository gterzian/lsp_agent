use serde::Serialize;
use crate::ConversationFragment;

const WEB_ENVIRONMENT_SYSTEM_PROMPT: &str = include_str!("../prompts/web-environment.md");

#[derive(Serialize)]
struct HistoryItem {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct WebRequest<'a> {
    system: &'a str,
    history: Vec<HistoryItem>,
    latest_user: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    apps: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    apps_note: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    open_documents: Option<&'a [String]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    active_document: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    docs_note: Option<&'a str>,
}

pub fn build_web_request(
    history: &[ConversationFragment],
    latest_user: &str,
    apps: Option<&[String]>,
    docs: Option<&DocsInfo>,
) -> String {
    let request = WebRequest {
        system: WEB_ENVIRONMENT_SYSTEM_PROMPT.trim_end(),
        history: render_history(history, false, false),
        latest_user,
        apps,
        apps_note: apps
            .as_ref()
            .map(|_| "The app list below is provided because you requested running apps."),
        open_documents: docs.map(|info| info.open_documents.as_slice()),
        active_document: docs.and_then(|info| info.active_document.as_deref()),
        docs_note: docs
            .as_ref()
            .map(|_| "The document list below is provided because you requested open documents."),
    };

    serde_json::to_string_pretty(&request).unwrap_or_else(|_| "{}".to_string())
}

fn render_history(
    history: &[ConversationFragment],
    include_apps_marker: bool,
    include_docs_marker: bool,
) -> Vec<HistoryItem> {
    let mut items: Vec<HistoryItem> = history
        .iter()
        .map(|fragment| match fragment {
            ConversationFragment::Assistant(content) => HistoryItem {
                role: "assistant",
                content: content.clone(),
            },
            ConversationFragment::User(content) => HistoryItem {
                role: "user",
                content: content.clone(),
            },
        })
        .collect();

    if include_apps_marker {
        items.push(HistoryItem {
            role: "assistant",
            content: "Assistant requested info on running apps.".to_string(),
        });
    }

    if include_docs_marker {
        items.push(HistoryItem {
            role: "assistant",
            content: "Assistant requested info on open documents.".to_string(),
        });
    }

    items
}

#[derive(Serialize, Clone)]
pub struct DocsInfo {
    pub open_documents: Vec<String>,
    pub active_document: Option<String>,
}
