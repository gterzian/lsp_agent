use automerge_repo::{ConnDirection, DocHandle, DocumentId, Repo, Storage, StorageError};
use autosurgeon::{Hydrate, Reconcile, hydrate, reconcile};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::time::{Duration, sleep};
use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use uuid::Uuid;

struct InferenceLspRequest;

impl tower_lsp::lsp_types::request::Request for InferenceLspRequest {
    type Params = InferenceParams;
    type Result = InferenceResult;
    const METHOD: &'static str = "custom/inference";
}

#[derive(Serialize, Deserialize, Debug)]
struct InferenceParams {
    request: String,
}

#[derive(Serialize, Deserialize, Debug)]
struct InferenceResult {
    response: String,
}

struct Backend {
    client: Client,
    doc_handle: DocHandle,
    agent_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        self.client
            .log_message(MessageType::INFO, "Server initializing...")
            .await;
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                execute_command_provider: Some(ExecuteCommandOptions {
                    commands: vec![
                        "lsp-agent.log-chat".to_string(),
                        "lsp-agent.active-doc".to_string(),
                    ],
                    ..Default::default()
                }),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.client
            .log_message(MessageType::INFO, "Server initialized!")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.should_exit = true;
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        if let Some(task) = self.agent_task.lock().await.take() {
            let _ = task.await;
        }
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "File opened")
            .await;
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text;

        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .text_documents
                .documents
                .insert(uri, DocumentContent { text });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();

        if let Some(change) = params.content_changes.last() {
            let text = change.text.clone();
            self.doc_handle.with_doc_mut(|doc| {
                let mut agent: LspAgent = hydrate(doc).unwrap();
                agent
                    .text_documents
                    .documents
                    .insert(uri, DocumentContent { text });
                let mut tx = doc.transaction();
                reconcile(&mut tx, &agent).unwrap();
                tx.commit();
            });
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.client
            .log_message(MessageType::INFO, "File closed")
            .await;
        let uri = params.text_document.uri.to_string();

        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.text_documents.documents.remove(&uri);
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> Result<Option<serde_json::Value>> {
        if params.command == "lsp-agent.log-chat" {
            if let Some(arg) = params.arguments.first().and_then(|v| v.as_str()) {
                let request_text = arg.to_string();
                let req_id = Id {
                    value: Uuid::new_v4().to_string(),
                };

                self.doc_handle.with_doc_mut(|doc| {
                    let mut agent: LspAgent = hydrate(doc).unwrap();
                    agent.requests.insert(
                        req_id.clone(),
                        ChatRequest {
                            content: request_text,
                        },
                    );
                    let mut tx = doc.transaction();
                    reconcile(&mut tx, &agent).unwrap();
                    tx.commit();
                });
            }

            self.client
                .log_message(
                    MessageType::INFO,
                    format!("Chat Request Received: {:?}", params.arguments),
                )
                .await;
        } else if params.command == "lsp-agent.active-doc" {
            if let Some(uri) = params.arguments.first().and_then(|v| v.as_str()) {
                let uri_string = uri.to_string();
                self.client
                    .log_message(
                        MessageType::INFO,
                        format!("Active doc changed to: {}", uri_string),
                    )
                    .await;

                self.doc_handle.with_doc_mut(|doc| {
                    let mut agent: LspAgent = hydrate(doc).unwrap();
                    agent.text_documents.active_document = Some(Uri { value: uri_string });
                    let mut tx = doc.transaction();
                    reconcile(&mut tx, &agent).unwrap();
                    tx.commit();
                });
            }
        }
        Ok(None)
    }
}

const PEER1_PORT: u16 = 2341;
const PEER2_PORT: u16 = 2342;

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct ChatRequest {
    content: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct ChatResponse {
    content: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct DocumentContent {
    text: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default, Hash, Eq)]
struct Id {
    value: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct Uri {
    value: String,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct DocumentManager {
    documents: HashMap<String, DocumentContent>,
    active_document: Option<Uri>,
}

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq, Default)]
struct LspAgent {
    requests: HashMap<Id, ChatRequest>,
    responses: HashMap<Id, ChatResponse>,
    text_documents: DocumentManager,
    webviews: DocumentManager,
    should_exit: bool,
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

/*
#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
struct InferenceRequest(String);

#[derive(Debug, Clone, Reconcile, Hydrate, PartialEq)]
struct InferenceResponse(String);
*/

struct NoStorage;

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

fn start_automerge_infrastructure(client: Client) -> (DocHandle, tokio::task::JoinHandle<()>) {
    let handle = Handle::current();

    // 1. Setup Server (Repo 1, Port 2341)
    let repo1 = Repo::new(None, Box::new(NoStorage));
    let repo_handle1 = repo1.run();

    // 2. Bootstrap Document
    let doc_handle = repo_handle1.new_document();
    let doc_id = doc_handle.document_id();

    doc_handle.with_doc_mut(|doc| {
        let mut tx = doc.transaction();
        let agent = LspAgent::default();
        reconcile(&mut tx, &agent).unwrap();
        tx.commit();
    });

    let main_task_doc_handle = doc_handle.clone();
    let main_task_repo_handle = repo_handle1.clone();
    let main_task_client = client.clone();

    // Spawn Main Logic Task
    let main_task = handle.spawn(async move {
        // Listener
        let repo_clone1 = main_task_repo_handle.clone();
        tokio::spawn(async move {
            let addr1 = format!("127.0.0.1:{}", PEER1_PORT);
            let listener = TcpListener::bind(addr1).await.unwrap();
            loop {
                if let Ok((socket, addr)) = listener.accept().await {
                    repo_clone1
                        .connect_tokio_io(addr, socket, ConnDirection::Incoming)
                        .await
                        .unwrap();
                }
            }
        });

        // Connect to Peer 2
        let repo_clone1 = main_task_repo_handle.clone();
        tokio::spawn(async move {
            let addr = format!("127.0.0.1:{}", PEER2_PORT);
            loop {
                match TcpStream::connect(&addr).await {
                    Ok(stream) => {
                        repo_clone1
                            .connect_tokio_io(addr, stream, ConnDirection::Outgoing)
                            .await
                            .unwrap();
                        break;
                    }
                    Err(_) => {
                        sleep(Duration::from_millis(500)).await;
                    }
                }
            }
        });

        // Agent Logic Loop
        loop {
            main_task_doc_handle.changed().await.unwrap();

            let (should_exit, pending_request) =
                main_task_doc_handle.with_doc(|doc| match hydrate::<_, LspAgent>(doc) {
                    Ok(agent) => (
                        agent.should_exit,
                        agent
                            .requests
                            .iter()
                            .find(|(id, _)| !agent.responses.contains_key(*id))
                            .map(|(id, req)| (id.clone(), req.content.clone())),
                    ),
                    Err(e) => {
                        eprintln!("Error in Agent Loop: {:?}", e);
                        (false, None)
                    }
                });

            if should_exit {
                Handle::current()
                    .spawn_blocking(|| {
                        main_task_repo_handle.stop().unwrap();
                    })
                    .await
                    .unwrap();
                break;
            }

            if let Some((req_id, req_str)) = pending_request {
                let response_str = {
                    let params = InferenceParams {
                        request: req_str.clone(),
                    };
                    match main_task_client
                        .send_request::<InferenceLspRequest>(params)
                        .await
                    {
                        Ok(res) => res.response,
                        Err(e) => {
                            eprintln!("LSP Inference Error: {:?}", e);
                            format!("Error: {:?}", e)
                        }
                    }
                };

                main_task_doc_handle.with_doc_mut(|doc| {
                    let mut agent: LspAgent = hydrate(doc).unwrap();
                    if !agent.responses.contains_key(&req_id) {
                        agent.responses.insert(
                            req_id,
                            ChatResponse {
                                content: response_str.clone(),
                            },
                        );
                        let mut tx = doc.transaction();
                        reconcile(&mut tx, &agent).unwrap();
                        tx.commit();
                    }
                });

                main_task_client
                    .log_message(
                        MessageType::INFO,
                        format!("Chat Response: {}", response_str),
                    )
                    .await;
            }
        }
    });

    // 5. Spawn Test Task (Simulate Peer 2)
    let doc_id_clone = doc_id.clone();
    handle.spawn(async move {
        let repo2 = Repo::new(None, Box::new(NoStorage));
        let repo_handle2 = repo2.run();
        let repo_clone2 = repo_handle2.clone();
        let addr2 = format!("127.0.0.1:{}", PEER2_PORT);

        // Listen as Peer 2
        tokio::spawn(async move {
            match TcpListener::bind(&addr2).await {
                Ok(listener) => loop {
                    if let Ok((socket, addr)) = listener.accept().await {
                        repo_clone2
                            .connect_tokio_io(addr, socket, ConnDirection::Incoming)
                            .await
                            .unwrap();
                    }
                },
                Err(e) => {
                    eprintln!("Failed to bind Peer 2: {:?}", e);
                }
            }
        });

        // Request Doc
        let doc_handle_peer2 = repo_handle2.request_document(doc_id_clone).await.unwrap();

        // Wait for sync (simple hacky wait)
        sleep(Duration::from_millis(2000)).await;

        let req_id = Id {
            value: Uuid::new_v4().to_string(),
        };

        doc_handle_peer2.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.requests.insert(
                req_id.clone(),
                ChatRequest {
                    content: "Hello World".to_string(),
                },
            );
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        // Watch for Response
        loop {
            doc_handle_peer2.changed().await.unwrap();
            let agent: LspAgent = doc_handle_peer2.with_doc(|doc| hydrate(doc).unwrap());
            if let Some(_resp) = agent.responses.get(&req_id) {
                break;
            }
        }
        repo_handle2.stop().unwrap();
    });

    (doc_handle, main_task)
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        let (doc_handle, task) = start_automerge_infrastructure(client.clone());
        Backend {
            client,
            doc_handle,
            agent_task: Mutex::new(Some(task)),
        }
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
