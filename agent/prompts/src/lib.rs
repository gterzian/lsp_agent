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
        use serde_json::Value;
        let history = vec![ConversationFragment::User("hello".to_string())];
        let request = build_web_request(&history, "test prompt", None, None);
        let parsed: Value = serde_json::from_str(&request).unwrap();

        // Should contain the system prompt and latest_user
        assert!(parsed["system"]
            .as_str()
            .unwrap()
            .contains("You are an expert web developer assistant"));
        assert_eq!(parsed["latest_user"].as_str().unwrap(), "test prompt");
        assert!(parsed["history"].is_array());

        // Optional fields should be absent
        assert!(parsed.get("apps").is_none());
        assert!(parsed.get("open_documents").is_none());
    }

    #[test]
    fn test_build_web_request_with_apps() {
        use serde_json::Value;
        let history = vec![ConversationFragment::Assistant(
            "previous response".to_string(),
        )];
        let apps = vec!["app1".to_string(), "app2".to_string()];
        let request = build_web_request(&history, "launch app", Some(&apps), None);
        let parsed: Value = serde_json::from_str(&request).unwrap();

        let apps_val = parsed
            .get("apps")
            .and_then(|v| v.as_array())
            .expect("apps should be an array");
        assert_eq!(apps_val[0].as_str().unwrap(), "app1");
        assert_eq!(apps_val[1].as_str().unwrap(), "app2");
        assert!(parsed.get("apps_note").is_some());
        assert!(parsed["history"].as_array().unwrap()[0]["content"]
            .as_str()
            .unwrap()
            .contains("previous response"));
    }

    #[test]
    fn test_build_web_request_with_docs() {
        use serde_json::Value;
        let history = vec![];
        let docs = DocsInfo {
            open_documents: vec!["file1.rs".to_string(), "file2.rs".to_string()],
            active_document: Some("file1.rs".to_string()),
        };
        let request = build_web_request(&history, "summarize", None, Some(&docs));
        let parsed: Value = serde_json::from_str(&request).unwrap();

        let docs_arr = parsed
            .get("open_documents")
            .and_then(|v| v.as_array())
            .expect("open_documents should be an array");
        assert!(docs_arr.iter().any(|v| v.as_str().unwrap() == "file1.rs"));
        assert!(parsed.get("docs_note").is_some());
        assert_eq!(
            parsed
                .get("active_document")
                .and_then(|v| v.as_str())
                .unwrap(),
            "file1.rs"
        );
    }

    #[test]
    fn test_build_web_request_with_all_options() {
        use serde_json::Value;
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
        let parsed: Value = serde_json::from_str(&request).unwrap();

        let hist = parsed.get("history").and_then(|v| v.as_array()).unwrap();
        assert!(hist
            .iter()
            .any(|i| i["content"].as_str().unwrap() == "user question"));
        assert_eq!(parsed["apps"][0].as_str().unwrap(), "todo app");
        assert_eq!(parsed["open_documents"][0].as_str().unwrap(), "main.rs");
        assert!(parsed.get("apps_note").is_some());
        assert!(parsed.get("docs_note").is_some());
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
