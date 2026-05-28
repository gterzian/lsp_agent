use crate::ConversationFragment;
use serde::Serialize;

const WEB_ENVIRONMENT_SYSTEM_PROMPT: &str = include_str!("../prompts/web-environment.md");
const MAKEPAD_ENVIRONMENT_SYSTEM_PROMPT: &str = include_str!("../prompts/makepad-environment.md");

#[derive(Serialize)]
struct HistoryItem {
    role: &'static str,
    content: String,
}

#[derive(Serialize)]
struct AppRequest<'a> {
    runtime: &'a str,
    system: &'a str,
    history: Vec<HistoryItem>,
    latest_user: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    standard_apps: Option<&'a [StandardAppInfo]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    standard_apps_note: Option<&'a str>,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    stored_values: Option<&'a [StoredValueInfo]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    stored_values_note: Option<&'a str>,
}

pub fn build_app_request(
    runtime: &str,
    history: &[ConversationFragment],
    latest_user: &str,
    apps: Option<&[String]>,
    docs: Option<&DocsInfo>,
    stored_values: Option<&[StoredValueInfo]>,
    standard_apps: Option<&[StandardAppInfo]>,
) -> String {
    let request = AppRequest {
        runtime,
        system: system_prompt_for_runtime(runtime).trim_end(),
        history: render_history(history, false, false),
        latest_user,
        standard_apps,
        standard_apps_note: standard_apps
            .as_ref()
            .map(|_| standard_apps_note_for_runtime(runtime)),
        apps,
        apps_note: apps.as_ref().map(|_| apps_note_for_runtime(runtime)),
        open_documents: docs.map(|info| info.open_documents.as_slice()),
        active_document: docs.and_then(|info| info.active_document.as_deref()),
        docs_note: docs
            .as_ref()
            .map(|_| "The document list below is provided because you requested open documents."),
        stored_values,
        stored_values_note: stored_values
            .as_ref()
            .map(|_| "The stored values list below is provided because you requested it."),
    };

    serde_json::to_string_pretty(&request).unwrap_or_else(|_| "{}".to_string())
}

fn system_prompt_for_runtime(runtime: &str) -> &'static str {
    match runtime {
        "makepad" => MAKEPAD_ENVIRONMENT_SYSTEM_PROMPT,
        _ => WEB_ENVIRONMENT_SYSTEM_PROMPT,
    }
}

fn apps_note_for_runtime(runtime: &str) -> &'static str {
    match runtime {
        "makepad" => {
            "The app list below is provided because you requested running apps. Each entry is the raw Splash body currently rendered in the persistent Makepad host."
        }
        _ => {
            "The app list below is provided because you requested running apps. Each entry is a running web app HTML document."
        }
    }
}

fn standard_apps_note_for_runtime(runtime: &str) -> &'static str {
    match runtime {
        "makepad" => {
            "The standard_apps list below contains named, well-written built-in Makepad apps. Prefer launch_standard_app when one clearly fits the user's request."
        }
        _ => {
            "The standard_apps list below contains named built-in apps available for this runtime."
        }
    }
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

#[derive(Serialize, Clone)]
pub struct StoredValueInfo {
    pub key: String,
    pub description: String,
}

#[derive(Serialize, Clone)]
pub struct StandardAppInfo {
    pub id: String,
    pub description: String,
}
