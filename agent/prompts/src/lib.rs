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
        history: render_history(history, apps.is_some(), docs.is_some()),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_web_request_basic() {
        let history = vec![ConversationFragment::User("hello".to_string())];
        let request = build_web_request(&history, "test prompt", None, None);

        // Should contain the system prompt
        assert!(request.contains("You are an expert web developer assistant"));
        assert!(request.contains("hello"));
        assert!(request.contains("test prompt"));

        // Should not contain optional fields - check that "apps" field is not in the JSON
        assert!(!request.contains("\"apps\":"));
        assert!(!request.contains("\"open_documents\":"));
    }

    #[test]
    fn test_build_web_request_with_docs() {
        let history = vec![];
        let docs = DocsInfo {
            open_documents: vec!["file1.rs".to_string(), "file2.rs".to_string()],
            active_document: Some("file1.rs".to_string()),
        };
        let request = build_web_request(&history, "summarize", None, Some(&docs));

        assert!(request.contains("file1.rs"));
        assert!(request.contains("file2.rs"));
        assert!(request.contains("open documents"));
    }

    #[test]
    fn test_build_web_request_with_all_options() {
        let history = vec![
            ConversationFragment::User("user question".to_string()),
            ConversationFragment::Assistant("assistant response".to_string()),
        ];
        let apps = vec!["todo app".to_string()];
        let docs = DocsInfo {
            open_documents: vec!["main.rs".to_string()],
            active_document: Some("main.rs".to_string()),
        };
        let request = build_web_request(&history, "help me", Some(&apps), Some(&docs));

        assert!(request.contains("user question"));
        assert!(request.contains("assistant response"));
        assert!(request.contains("todo app"));
        assert!(request.contains("main.rs"));
        assert!(request.contains("running apps"));
        assert!(request.contains("open documents"));
    }

    #[test]
    fn test_render_history_basic() {
        let history = vec![
            ConversationFragment::User("user message".to_string()),
            ConversationFragment::Assistant("assistant message".to_string()),
        ];

        let items = render_history(&history, false, false);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].role, "user");
        assert_eq!(items[0].content, "user message");
        assert_eq!(items[1].role, "assistant");
        assert_eq!(items[1].content, "assistant message");
    }

    #[test]
    fn test_render_history_with_markers() {
        let history = vec![ConversationFragment::User("test".to_string())];

        let items = render_history(&history, true, true);
        assert_eq!(items.len(), 3);
        assert_eq!(items[1].role, "assistant");
        assert_eq!(
            items[1].content,
            "Assistant requested info on running apps."
        );
        assert_eq!(items[2].role, "assistant");
        assert_eq!(
            items[2].content,
            "Assistant requested info on open documents."
        );
    }

    #[test]
    fn test_render_history_empty() {
        let history = vec![];
        let items = render_history(&history, false, false);
        assert_eq!(items.len(), 0);
    }

    #[test]
    fn test_docs_info_serialization() {
        let docs = DocsInfo {
            open_documents: vec!["a.rs".to_string(), "b.rs".to_string()],
            active_document: Some("a.rs".to_string()),
        };

        let json = serde_json::to_string(&docs).unwrap();
        assert!(json.contains("a.rs"));
        assert!(json.contains("b.rs"));
    }
}
