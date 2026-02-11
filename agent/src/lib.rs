mod document;
pub mod prompts;

pub use document::{
    AgentRequest, AgentResponse, ConversationFragment, DocumentContent, DocumentManager, Id,
    LspAgent, NoStorage, StoredValue, Uri,
};

use automerge_repo::{ConnDirection, DocHandle, DocumentId, Repo, RepoHandle};
use autosurgeon::{hydrate, reconcile};
use serde::Deserialize;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::runtime::Handle;
use tokio::sync::{mpsc, Mutex, oneshot};
use tokio::time::{sleep, Duration};
use traits::{InferenceClient, Web, WebAgent, WorkspaceAgent};
use uuid::Uuid;

fn find_repo_root(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    for ancestor in exe_path.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&candidate)
            && contents.contains("[workspace]")
        {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

const PEER1_PORT: u16 = 2341;
const PEER2_PORT: u16 = 2342;
const DEFAULT_TOOL_MAX_ITERATIONS: usize = 3;

#[derive(Deserialize, Debug)]
struct ToolResponse {
    action: String,
    message: Option<String>,
    app: Option<String>,
}

pub fn start_infra(client: Arc<dyn InferenceClient>) -> Box<dyn WorkspaceAgent> {
    let (doc_handle, task, chat_tx) = start_automerge_infrastructure(client);
    let child = spawn_web_client();

    Box::new(AutomergeAgent {
        doc_handle,
        agent_task: Mutex::new(Some(task)),
        web_child: Mutex::new(child),
        chat_tx,
    })
}

struct ChatRequest {
    content: String,
    model: Option<String>,
    responder: oneshot::Sender<Option<String>>,
}

struct AutomergeAgent {
    doc_handle: DocHandle,
    agent_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    web_child: Mutex<Option<Child>>,
    chat_tx: mpsc::Sender<ChatRequest>,
}

/// Web sink used in the server process to enqueue web responses into the shared doc.
///
/// This does not call any web APIs directly; it only writes `AgentResponse` entries
/// that the web process will observe and handle.
struct DocWebSink {
    doc_handle: DocHandle,
}

/// Web agent used in the web client process to enqueue requests into the shared doc.
///
/// The web client cannot call the inference client directly, so it records
/// `AgentRequest` entries that the server process will consume.
pub struct DocWebAgent {
    doc_handle: DocHandle,
}

impl DocWebAgent {
    pub fn new(doc_handle: DocHandle) -> Self {
        Self { doc_handle }
    }
}

/// Starts the web backend loop in the web client process.
///
/// This connects to the shared document, watches for `AgentResponse` entries,
/// and forwards them to the provided `Web` implementation (which owns the UI/webview).
/// It returns a `WebAgent` that writes requests into the shared document for the
/// server process to handle.
pub async fn start_web_backend(web: Arc<dyn Web>) -> (Box<dyn WebAgent>, mpsc::Receiver<()>) {
    let doc_handle = setup_web_doc().await;
    let agent = DocWebAgent::new(doc_handle.clone());
    let (exit_tx, exit_rx) = mpsc::channel(1);

    tokio::spawn(async move {
        loop {
            if doc_handle.changed().await.is_err() {
                let _ = exit_tx.send(()).await;
                break;
            }

            if handle_web_doc_change(&doc_handle, web.as_ref()).await {
                let _ = exit_tx.send(()).await;
                break;
            }
        }
    });

    (Box::new(agent), exit_rx)
}

#[async_trait::async_trait]
impl Web for DocWebSink {
    async fn launch_app(&self, id: String, content: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .webviews
                .documents
                .insert(id.clone(), DocumentContent { text: content.clone() });
            agent.responses.push(AgentResponse::WebApp {
                id,
                content,
            });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn handle_inference_response(&self, app_id: String, content: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.responses.push(AgentResponse::Inference { app_id, content });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }
}

#[async_trait::async_trait]
impl WorkspaceAgent for AutomergeAgent {
    async fn shutdown(&self) {
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

        if let Some(mut child) = self.web_child.lock().await.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    async fn did_open(&self, uri: String, text: String) {
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

    async fn did_change(&self, uri: String, text: String) {
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

    async fn did_close(&self, uri: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.text_documents.documents.remove(&uri);
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn set_active_document(&self, uri: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.text_documents.active_document = Some(Uri { value: uri });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn chat_request(&self, content: String, model: Option<String>) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        let req = ChatRequest {
            content,
            model,
            responder: tx,
        };
        if self.chat_tx.send(req).await.is_err() {
            return None;
        }
        rx.await.ok().flatten()
    }
}

#[async_trait::async_trait]
impl WebAgent for DocWebAgent {
    async fn app_inference_request(&self, content: String, app_id: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.requests.push(AgentRequest::Inference { content, app_id });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn read_document(&self, uri: String) -> String {
        self.doc_handle.with_doc(|doc| {
            let agent: LspAgent = hydrate(doc).unwrap();
            agent
                .text_documents
                .documents
                .get(&uri)
                .map(|doc| doc.text.clone())
                .unwrap_or_default()
        })
    }

    async fn close_app(&self, app_id: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.webviews.documents.remove(&app_id);
            agent
                .conversation_history
                .push(ConversationFragment::Assistant(format!(
                    "App closed: {}",
                    app_id
                )));
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn store_value(&self, key: String, value: String, description: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .stored_values
                .insert(key, StoredValue { value, description });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn read_value(&self, key: String) -> Option<String> {
        self.doc_handle.with_doc(|doc| {
            let agent: LspAgent = hydrate(doc).unwrap();
            agent.stored_values.get(&key).map(|v| v.value.clone())
        })
    }
}

async fn setup_web_doc() -> DocHandle {
    let repo = Repo::new(None, Box::new(NoStorage));
    let repo_handle = repo.run();
    listen_peer2(repo_handle.clone());

    let doc_id = wait_for_doc_id().await;
    println!("Found Doc ID: {}", doc_id);

    repo_handle.request_document(doc_id.clone()).await.unwrap()
}

fn listen_peer2(repo_handle: RepoHandle) {
    let addr = format!("127.0.0.1:{}", PEER2_PORT);
    tokio::spawn(async move {
        match TcpListener::bind(&addr).await {
            Ok(listener) => loop {
                if let Ok((socket, addr)) = listener.accept().await {
                    repo_handle
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
}

async fn wait_for_doc_id() -> DocumentId {
    println!("Waiting for doc_id from HTTP...");
    let doc_id_str = loop {
        match reqwest::get("http://127.0.0.1:2348/doc_id").await {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    break text.trim().to_string();
                }
            }
            Err(_) => {
                sleep(Duration::from_millis(1000)).await;
            }
        }
    };

    doc_id_str.parse().expect("Failed to parse document ID")
}

async fn handle_web_doc_change(doc_handle: &DocHandle, web: &dyn Web) -> bool {
    let (should_exit, should_handle_response) = doc_handle.with_doc(|doc| {
        let agent: LspAgent = hydrate(doc).unwrap();
        let handle = match agent.responses.first() {
            Some(AgentResponse::Chat(_)) => false,
            Some(_) => true,
            None => false,
        };
        (agent.should_exit, handle)
    });

    if should_exit {
        return true;
    }

    if should_handle_response {
        let response_enum = take_response(doc_handle);
        if let Some(resp) = response_enum {
            handle_web_response(web, resp).await;
        }
    }

    false
}

fn take_response(doc_handle: &DocHandle) -> Option<AgentResponse> {
    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = hydrate(doc).unwrap();
        let resp = if !agent.responses.is_empty() {
            Some(agent.responses.remove(0))
        } else {
            None
        };

        if resp.is_some() {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        }

        resp
    })
}

async fn handle_web_response(web: &dyn Web, resp: AgentResponse) {
    match resp {
        AgentResponse::WebApp { id, content } => {
            web.launch_app(id, content).await;
        }
        AgentResponse::Chat(_) => {
            debug_assert!(false, "Web backend should not consume chat responses");
        }
        AgentResponse::Inference { app_id, content } => {
            web.handle_inference_response(app_id, content).await;
        }
    }
}

fn spawn_web_client() -> Option<Child> {
    let exe_path = std::env::current_exe().expect("Failed to get current exe path");
    let project_root = find_repo_root(&exe_path).unwrap_or_else(|| {
        exe_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .expect("Failed to find project root")
            .to_path_buf()
    });

    let mut candidates = Vec::new();
    if let Ok(override_path) = std::env::var("LSP_AGENT_WEB_BINARY") {
        candidates.push(std::path::PathBuf::from(override_path));
    }
    candidates.push(project_root.join("target/debug/web"));
    candidates.push(project_root.join("web/target/debug/web"));

    let web_binary = match candidates.into_iter().find(|path| path.exists()) {
        Some(path) => path,
        None => {
            eprintln!(
                "[LSP Agent] Web client binary not found. Rebuild the web crate or set LSP_AGENT_WEB_BINARY."
            );
            return None;
        }
    };

    let mut child = Command::new(&web_binary);
    child.stdout(std::process::Stdio::null());
    child.stderr(std::process::Stdio::inherit());
    match child.spawn() {
        Ok(child) => Some(child),
        Err(err) => {
            eprintln!(
                "[LSP Agent] Failed to spawn web client at {}: {:?}",
                web_binary.display(),
                err
            );
            None
        }
    }
}

/// Starts the server-side infrastructure in the LSP/server process.
///
/// This owns the inference client, consumes `AgentRequest` entries from the shared doc,
/// and writes `AgentResponse` entries that the web client will handle.
fn start_automerge_infrastructure(
    client: Arc<dyn InferenceClient>,
) -> (DocHandle, tokio::task::JoinHandle<()>, mpsc::Sender<ChatRequest>) {
    let handle = Handle::current();

    let repo1 = Repo::new(None, Box::new(NoStorage));
    let repo_handle1 = repo1.run();

    let doc_handle = repo_handle1.new_document();
    let doc_id = doc_handle.document_id();

    let doc_id_str = doc_id.to_string();
    tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/doc_id",
            axum::routing::get(move || async move { doc_id_str }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:2348")
            .await
            .unwrap();
        axum::serve(listener, app).await.unwrap();
    });

    doc_handle.with_doc_mut(|doc| {
        let mut tx = doc.transaction();
        let agent = LspAgent::default();
        reconcile(&mut tx, &agent).unwrap();
        tx.commit();
    });

    let main_task_doc_handle = doc_handle.clone();
    let web_sink: Arc<dyn Web> = Arc::new(DocWebSink {
        doc_handle: doc_handle.clone(),
    });
    let main_task_repo_handle = repo_handle1.clone();
    let main_task_client = client.clone();

    let (chat_tx, mut chat_rx) = mpsc::channel::<ChatRequest>(32);
    let main_task = handle.spawn(async move {
        spawn_peer_connections(main_task_repo_handle.clone());

        loop {
            tokio::select! {
                changed = main_task_doc_handle.changed() => {
                    if changed.is_err() {
                        break;
                    }

                    let (should_exit, pending_request, active_model) = check_agent_state(&main_task_doc_handle);

                    if should_exit {
                        perform_shutdown(&main_task_client, &main_task_repo_handle).await;
                        break;
                    }

                    if let Some(req) = pending_request {
                        handle_inference_request(req, &main_task_client, active_model, web_sink.as_ref()).await;
                    }
                }
                Some(chat_req) = chat_rx.recv() => {
                    handle_chat_request(chat_req, &main_task_doc_handle, &main_task_client, web_sink.as_ref()).await;
                }
                else => {
                    break;
                }
            }
        }
    });

    (doc_handle, main_task, chat_tx)
}


fn check_agent_state(doc_handle: &DocHandle) -> (bool, Option<AgentRequest>, Option<String>) {
    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = hydrate(doc).unwrap();
        let req = if !agent.requests.is_empty() {
            Some(agent.requests.remove(0))
        } else {
            None
        };

        if req.is_some() {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        }

        (agent.should_exit, req, agent.active_model)
    })
}

async fn perform_shutdown(client: &Arc<dyn InferenceClient>, repo_handle: &RepoHandle) {
    client.notify_shutdown().await;
    let repo_handle = repo_handle.clone();
    Handle::current()
        .spawn_blocking(move || {
            repo_handle.stop().unwrap();
        })
        .await
        .unwrap();
}

async fn handle_inference_request(
    req: AgentRequest,
    client: &Arc<dyn InferenceClient>,
    active_model: Option<String>,
    web_sink: &dyn Web,
) {
    match req {
        AgentRequest::Inference { content, app_id } => {
            let response_str = call_inference(
                client.as_ref(),
                content,
                active_model,
            )
            .await;
            web_sink
                .handle_inference_response(app_id, response_str)
                .await;
        }
    }
}

fn spawn_peer_connections(repo_handle: RepoHandle) {
    let repo_clone1 = repo_handle.clone();
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

    let repo_clone2 = repo_handle.clone();
    tokio::spawn(async move {
        let addr = format!("127.0.0.1:{}", PEER2_PORT);
        loop {
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    repo_clone2
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
}

async fn handle_chat_request(
    chat_req: ChatRequest,
    doc_handle: &DocHandle,
    client: &Arc<dyn InferenceClient>,
    web_sink: &dyn Web,
) {
    let ChatRequest {
        content: latest_user,
        model: model_hint,
        responder,
    } = chat_req;

    let (mut history, running_apps, docs_info, stored_values_info) = doc_handle.with_doc(|doc| {
        let agent: LspAgent = hydrate(doc).unwrap();
        (
            agent.conversation_history.clone(),
            collect_apps(&agent.webviews),
            collect_docs(&agent.text_documents),
            collect_stored_values(&agent.stored_values),
        )
    });

    let initial_history_len = history.len();

    let mut apps_payload: Option<Vec<String>> = None;
    let mut docs_payload: Option<prompts::DocsInfo> = None;
    let mut stored_values_payload: Option<Vec<prompts::StoredValueInfo>> = None;
    let mut response_message: Option<String> = None;
    let mut launched_app: Option<String> = None;
    let mut did_nothing = false;

    let mut current_prompt_user = latest_user.clone();
    let mut pushed_user_message = false;

    let max_iterations = std::env::var("LSP_AGENT_TOOL_MAX_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_TOOL_MAX_ITERATIONS);

    for _ in 0..max_iterations {
        let request_text = prompts::build_web_request(
            &history,
            &current_prompt_user,
            apps_payload.as_deref(),
            docs_payload.as_ref(),
            stored_values_payload.as_deref(),
        );
        let tool_response_str =
            call_inference(client.as_ref(), request_text, model_hint.clone())
                .await;
        let tool_response = parse_tool_response(&tool_response_str);

        let mut next_turn_reason: Option<String> = None;

        match tool_response.action.as_str() {
            "answer" => {
                response_message = tool_response.message;
                break;
            }
            "nothing" => {
                did_nothing = true;
                break;
            }
            "launch_app" => {
                launched_app = tool_response.app;
                break;
            }
            "list_apps" => {
                if apps_payload.is_some() {
                    response_message = Some(
                        "App list was already provided, but the assistant requested it again without concluding."
                            .to_string(),
                    );
                    break;
                }
                apps_payload = Some(running_apps.clone());
                next_turn_reason = Some("Assistant requested info on running apps.".to_string());
            }
            "list_docs" => {
                if docs_payload.is_some() {
                    response_message = Some(
                        "Document list was already provided, but the assistant requested it again without concluding."
                            .to_string(),
                    );
                    break;
                }
                docs_payload = Some(docs_info.clone());
                next_turn_reason = Some("Assistant requested info on open documents.".to_string());
            }
            "list_app_values" => {
                if stored_values_payload.is_some() {
                    response_message = Some(
                        "Stored values list was already provided, but the assistant requested it again without concluding."
                            .to_string(),
                    );
                    break;
                }
                stored_values_payload = Some(stored_values_info.clone());
                next_turn_reason = Some("Assistant requested info on stored values.".to_string());
            }
            _ => {
                response_message = Some(tool_response_str);
            }
        }

        if let Some(reason) = next_turn_reason {
            if !pushed_user_message && !current_prompt_user.is_empty() {
                history.push(ConversationFragment::User(current_prompt_user.clone()));
                current_prompt_user.clear();
                pushed_user_message = true;
            }
            history.push(ConversationFragment::Assistant(reason));
            continue;
        }
    }

    if !did_nothing && launched_app.is_none() && response_message.is_none() {
        response_message = Some(
            "No actionable response was produced. Please retry or rephrase."
                .to_string(),
        );
    }

    let launched_app_for_doc = launched_app.clone();
    let did_launch_app = launched_app.is_some();
    // did_request_docs and did_request_apps removed as we use history diff

    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = hydrate(doc).unwrap();
        if let Some(model) = model_hint.clone() {
            agent.active_model = Some(model);
        }

        // 1. Add any history accumulated during tool use (User messages + Assistant markers)
        let new_fragments: Vec<ConversationFragment> = history
            .iter()
            .skip(initial_history_len)
            .cloned()
            .collect();
        agent.conversation_history.extend(new_fragments);

        // 2. Ensuring user message is present if not already in history (e.g. immediate answer/launch)
        if !pushed_user_message
            && !latest_user.is_empty()
            && (did_launch_app || response_message.is_some())
        {
            agent
                .conversation_history
                .push(ConversationFragment::User(latest_user.clone()));
        }

        // 3. Add final response
        if let Some(message) = response_message.clone() {
            agent
                .conversation_history
                .push(ConversationFragment::Assistant(message));
        }

        let mut tx = doc.transaction();
        reconcile(&mut tx, &agent).unwrap();
        tx.commit();
    });

    if let Some(app) = launched_app_for_doc {
        let app_id = format!("app-{}", Uuid::new_v4());
        web_sink
            .launch_app(app_id.clone(), app.clone())
            .await;
    }

    if let Some(message) = response_message {
        let _ = responder.send(Some(message));
    } else {
        let _ = responder.send(None);
    }
}

async fn call_inference(
    client: &dyn InferenceClient,
    request: String,
    model: Option<String>,
) -> String {
    match client.inference(request, model).await {
        Ok(res) => res,
        Err(e) => format!("Error: {}", e),
    }
}

fn parse_tool_response(response: &str) -> ToolResponse {
    match serde_json::from_str::<ToolResponse>(response) {
        Ok(mut parsed) => {
            if parsed.action == "answer" && parsed.message.is_none() {
                parsed.message = Some(response.to_string());
            }
            parsed
        }
        Err(_) => ToolResponse {
            action: "answer".to_string(),
            message: Some(response.to_string()),
            app: None,
        },
    }
}

fn collect_apps(manager: &DocumentManager) -> Vec<String> {
    manager
        .documents
        .values()
        .map(|doc| doc.text.clone())
        .collect()
}

fn collect_docs(manager: &DocumentManager) -> prompts::DocsInfo {
    let mut open_documents: Vec<String> = manager.documents.keys().cloned().collect();
    open_documents.sort();
    let active_document = manager
        .active_document
        .as_ref()
        .map(|uri| uri.value.clone());
    if let Some(active) = &active_document
        && !open_documents.contains(active)
    {
        open_documents.push(active.clone());
    }
    prompts::DocsInfo {
        open_documents,
        active_document,
    }
}

fn collect_stored_values(
    values: &std::collections::HashMap<String, StoredValue>,
) -> Vec<prompts::StoredValueInfo> {
    values
        .iter()
        .map(|(k, v)| prompts::StoredValueInfo {
            key: k.clone(),
            description: v.description.clone(),
        })
        .collect()
}
