use serde::Serialize;
use shared_document::ConversationFragment;

const WEB_ENVIRONMENT_SYSTEM_PROMPT: &str = include_str!("../web-environment.md");

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
}

pub fn build_web_request(
    history: &[ConversationFragment],
    latest_user: &str,
    apps: Option<&[String]>,
) -> String {
    let request = WebRequest {
        system: WEB_ENVIRONMENT_SYSTEM_PROMPT.trim_end(),
        history: render_history(history, apps.is_some()),
        latest_user,
        apps,
        apps_note: apps
            .as_ref()
            .map(|_| "The app list below is provided because you requested running apps."),
    };

    serde_json::to_string_pretty(&request).unwrap_or_else(|_| "{}".to_string())
}

fn render_history(history: &[ConversationFragment], include_apps_marker: bool) -> Vec<HistoryItem> {
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

    items
}
