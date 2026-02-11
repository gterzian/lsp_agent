use automerge_repo::{DocumentId, Storage, StorageError};
use autosurgeon::{Hydrate, Reconcile};
use futures::future::BoxFuture;
use std::collections::HashMap;

pub struct NoStorage;

impl Storage for NoStorage {
    fn get(
        &self,
        _id: DocumentId,
    ) -> BoxFuture<'static, std::result::Result<Option<Vec<u8>>, StorageError>> {
        Box::pin(futures::future::ready(Ok(None)))
    }

    fn list_all(&self) -> BoxFuture<'static, std::result::Result<Vec<DocumentId>, StorageError>> {
        Box::pin(futures::future::ready(Ok(vec![])))
    }

    fn append(
        &self,
        _id: DocumentId,
        _changes: Vec<u8>,
    ) -> BoxFuture<'static, std::result::Result<(), StorageError>> {
        Box::pin(futures::future::ready(Ok(())))
    }

    fn compact(
        &self,
        _id: DocumentId,
        _full_doc: Vec<u8>,
    ) -> BoxFuture<'static, std::result::Result<(), StorageError>> {
        Box::pin(futures::future::ready(Ok(())))
    }
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub enum AgentRequest {
    Inference { content: String, app_id: String },
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub enum AgentResponse {
    Chat(String),
    Inference { app_id: String, content: String },
    WebApp { id: String, content: String },
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
pub enum ConversationFragment {
    Assistant(String),
    User(String),
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct DocumentContent {
    pub text: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default, Hash, Eq)]
pub struct Id {
    pub value: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct Uri {
    pub value: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct DocumentManager {
    pub documents: HashMap<String, DocumentContent>,
    pub active_document: Option<Uri>,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct StoredValue {
    pub value: String,
    pub description: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct LspAgent {
    pub requests: Vec<AgentRequest>,
    pub responses: Vec<AgentResponse>,
    pub text_documents: DocumentManager,
    pub webviews: DocumentManager,
    pub should_exit: bool,
    pub active_model: Option<String>,
    pub conversation_history: Vec<ConversationFragment>,
    pub stored_values: HashMap<String, StoredValue>,
}

impl std::fmt::Display for Id {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl AsRef<str> for Id {
    fn as_ref(&self) -> &str {
        &self.value
    }
}

impl From<String> for Id {
    fn from(s: String) -> Self {
        Id { value: s }
    }
}

impl std::str::FromStr for Id {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
        Ok(Id {
            value: s.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autosurgeon::{hydrate, reconcile};

    #[test]
    fn test_lsp_agent_default() {
        let agent = LspAgent::default();
        assert!(agent.requests.is_empty());
        assert!(agent.responses.is_empty());
        assert!(agent.text_documents.documents.is_empty());
        assert!(agent.webviews.documents.is_empty());
        assert!(!agent.should_exit);
        assert!(agent.active_model.is_none());
        assert!(agent.conversation_history.is_empty());
        assert!(agent.stored_values.is_empty());
    }

    #[test]
    fn test_lsp_agent_serialization() {
        let mut doc = automerge::AutoCommit::new();
        let mut agent = LspAgent::default();
        agent.requests.push(AgentRequest::Inference {
            content: "test".to_string(),
            app_id: "app1".to_string(),
        });
        agent
            .responses
            .push(AgentResponse::Chat("response".to_string()));
        agent.text_documents.documents.insert(
            "file.rs".to_string(),
            DocumentContent {
                text: "code".to_string(),
            },
        );
        agent.stored_values.insert(
            "key1".to_string(),
            StoredValue {
                value: "value1".to_string(),
                description: "desc1".to_string(),
            },
        );

        reconcile(&mut doc, &agent).unwrap();

        let hydrated: LspAgent = hydrate(&doc).unwrap();
        assert_eq!(agent, hydrated);
    }

    #[test]
    fn test_agent_request_serialization() {
        let mut doc = automerge::AutoCommit::new();
        let request = AgentRequest::Inference {
            content: "test content".to_string(),
            app_id: "test_app".to_string(),
        };

        reconcile(&mut doc, &request).unwrap();
        let hydrated: AgentRequest = hydrate(&doc).unwrap();
        assert_eq!(request, hydrated);
    }

    #[test]
    fn test_agent_response_serialization() {
        let mut doc = automerge::AutoCommit::new();

        // Test Chat response
        let chat_response = AgentResponse::Chat("hello".to_string());
        reconcile(&mut doc, &chat_response).unwrap();
        let hydrated: AgentResponse = hydrate(&doc).unwrap();
        assert_eq!(chat_response, hydrated);

        // Test Inference response
        let mut doc2 = automerge::AutoCommit::new();
        let inference_response = AgentResponse::Inference {
            app_id: "app1".to_string(),
            content: "result".to_string(),
        };
        reconcile(&mut doc2, &inference_response).unwrap();
        let hydrated2: AgentResponse = hydrate(&doc2).unwrap();
        assert_eq!(inference_response, hydrated2);

        // Test WebApp response
        let mut doc3 = automerge::AutoCommit::new();
        let webapp_content = String::from("<html></html>");
        let webapp_response = AgentResponse::WebApp {
            id: String::from("app1"),
            content: webapp_content.clone(),
        };
        reconcile(&mut doc3, &webapp_response).unwrap();
        let hydrated3: AgentResponse = hydrate(&doc3).unwrap();
        assert_eq!(webapp_response, hydrated3);
    }

    #[test]
    fn test_conversation_fragment_serialization() {
        let mut doc = automerge::AutoCommit::new();

        let user_msg = String::from("user message");
        let user_fragment = ConversationFragment::User(user_msg.clone());
        reconcile(&mut doc, &user_fragment).unwrap();
        let hydrated: ConversationFragment = hydrate(&doc).unwrap();
        assert_eq!(user_fragment, hydrated);

        let mut doc2 = automerge::AutoCommit::new();
        let assistant_msg = String::from("assistant message");
        let assistant_fragment = ConversationFragment::Assistant(assistant_msg.clone());
        reconcile(&mut doc2, &assistant_fragment).unwrap();
        let hydrated2: ConversationFragment = hydrate(&doc2).unwrap();
        assert_eq!(assistant_fragment, hydrated2);
    }

    #[test]
    fn test_document_manager_serialization() {
        let mut doc = automerge::AutoCommit::new();
        let mut manager = DocumentManager::default();
        manager.documents.insert(
            "doc1".to_string(),
            DocumentContent {
                text: "content1".to_string(),
            },
        );
        manager.documents.insert(
            "doc2".to_string(),
            DocumentContent {
                text: "content2".to_string(),
            },
        );
        manager.active_document = Some(Uri {
            value: "doc1".to_string(),
        });

        reconcile(&mut doc, &manager).unwrap();
        let hydrated: DocumentManager = hydrate(&doc).unwrap();
        assert_eq!(manager, hydrated);
    }

    #[test]
    fn test_stored_value_serialization() {
        let mut doc = automerge::AutoCommit::new();
        let value_str = String::from("test_value");
        let desc_str = String::from("test description");
        let stored_value = StoredValue {
            value: value_str.clone(),
            description: desc_str.clone(),
        };

        reconcile(&mut doc, &stored_value).unwrap();
        let hydrated: StoredValue = hydrate(&doc).unwrap();
        assert_eq!(stored_value, hydrated);
    }

    #[test]
    fn test_id_conversions() {
        let id = Id::from("test".to_string());
        assert_eq!(id.value, "test");
        assert_eq!(id.as_ref(), "test");

        let id2: Id = "test2".parse().unwrap();
        assert_eq!(id2.value, "test2");
    }

    #[test]
    fn test_no_storage() {
        let storage = NoStorage;
        // NoStorage should not store anything
        let result = futures::executor::block_on(storage.get(automerge_repo::DocumentId::random()));
        assert!(matches!(result, Ok(None)));
    }
}
