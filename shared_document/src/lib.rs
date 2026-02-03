use autosurgeon::{Hydrate, Reconcile};
use std::collections::HashMap;
use automerge_repo::{DocumentId, Storage, StorageError};
use futures::future::BoxFuture;

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

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct ChatRequest {
    pub content: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
pub struct ChatResponse {
    pub content: String,
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
pub struct LspAgent {
    pub requests: Vec<ChatRequest>,
    pub responses: Vec<ChatResponse>,
    pub text_documents: DocumentManager,
    pub webviews: DocumentManager,
    pub should_exit: bool,
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
