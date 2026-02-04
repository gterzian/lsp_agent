mod document;
pub mod prompts;

pub use document::{
    AgentRequest, AgentResponse, ConversationFragment, DocumentContent, DocumentManager, Id,
    LspAgent, NoStorage, Uri,
};

use automerge_repo::{ConnDirection, DocHandle, Repo};
use autosurgeon::{hydrate, reconcile};
use serde::Deserialize;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::runtime::Handle;
use tokio::sync::Mutex;
use tokio::time::{sleep, Duration};
use traits::{Agent, AgentClient};
use uuid::Uuid;

fn find_repo_root(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    for ancestor in exe_path.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&candidate) {
            if contents.contains("[workspace]") {
                return Some(ancestor.to_path_buf());
            }
        }
    }
    None
}

const PEER1_PORT: u16 = 2341;
const PEER2_PORT: u16 = 2342;

#[derive(Deserialize, Debug)]
struct ToolResponse {
    action: String,
    message: Option<String>,
    app: Option<String>,
}

pub fn start_infra(client: Arc<dyn AgentClient>) -> Box<dyn Agent> {
    let (doc_handle, task) = start_automerge_infrastructure(client);
    let child = spawn_web_client();

    Box::new(AutomergeAgent {
        doc_handle,
        agent_task: Mutex::new(Some(task)),
        web_child: Mutex::new(child),
    })
}

struct AutomergeAgent {
    doc_handle: DocHandle,
    agent_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    web_child: Mutex<Option<Child>>,
}

#[async_trait::async_trait]
impl Agent for AutomergeAgent {
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

    async fn chat_request(&self, content: String, model: Option<String>) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .requests
                .push(AgentRequest::Chat { content, model });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
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

fn start_automerge_infrastructure(
    client: Arc<dyn AgentClient>,
) -> (DocHandle, tokio::task::JoinHandle<()>) {
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
    let main_task_repo_handle = repo_handle1.clone();
    let main_task_client = client.clone();

    let main_task = handle.spawn(async move {
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

        loop {
            main_task_doc_handle.changed().await.unwrap();

            let (should_exit, pending_request, active_model) =
                main_task_doc_handle.with_doc_mut(|doc| {
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
                });

            if should_exit {
                main_task_client.notify_shutdown().await;
                Handle::current()
                    .spawn_blocking(|| {
                        main_task_repo_handle.stop().unwrap();
                    })
                    .await
                    .unwrap();
                break;
            }

            if let Some(req) = pending_request {
                let (req_str, is_chat, model_hint, _app_id) = match req {
                    AgentRequest::Chat { content, model } => (content, true, model, String::new()),
                    AgentRequest::Inference { content, app_id } => {
                        (content, false, active_model.clone(), app_id)
                    }
                };

                if is_chat {
                    let (history, running_apps) = main_task_doc_handle.with_doc_mut(|doc| {
                        let agent: LspAgent = hydrate(doc).unwrap();
                        (
                            agent.conversation_history.clone(),
                            collect_apps(&agent.webviews),
                        )
                    });
                    let docs_info = main_task_doc_handle.with_doc_mut(|doc| {
                        let agent: LspAgent = hydrate(doc).unwrap();
                        collect_docs(&agent.text_documents)
                    });

                    let latest_user = req_str.clone();
                    let mut apps_payload: Option<Vec<String>> = None;
                    let mut docs_payload: Option<prompts::DocsInfo> = None;
                    let mut response_message: Option<String> = None;
                    let mut launched_app: Option<String> = None;

                    for _ in 0..3 {
                        let request_text = prompts::build_web_request(
                            &history,
                            &latest_user,
                            apps_payload.as_deref(),
                            docs_payload.as_ref(),
                        );
                        let tool_response_str =
                            call_inference(main_task_client.as_ref(), request_text, model_hint.clone())
                                .await;
                        let tool_response = parse_tool_response(&tool_response_str);

                        match tool_response.action.as_str() {
                            "answer" => {
                                response_message = tool_response.message;
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
                                continue;
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
                                continue;
                            }
                            _ => {
                                response_message = Some(tool_response_str);
                                break;
                            }
                        }
                    }

                    if launched_app.is_none() && response_message.is_none() {
                        response_message = Some(
                            "No actionable response was produced. Please retry or rephrase."
                                .to_string(),
                        );
                    }

                    main_task_doc_handle.with_doc_mut(|doc| {
                        let mut agent: LspAgent = hydrate(doc).unwrap();
                        if let Some(model) = model_hint.clone() {
                            agent.active_model = Some(model);
                        }
                        if let Some(app) = launched_app {
                            let app_id = format!("app-{}", Uuid::new_v4());
                            agent
                                .webviews
                                .documents
                                .insert(app_id.clone(), DocumentContent { text: app.clone() });
                            agent.responses.push(AgentResponse::WebApp {
                                id: app_id.clone(),
                                content: app,
                            });
                        } else if let Some(message) = response_message {
                            if !latest_user.is_empty() {
                                agent
                                    .conversation_history
                                    .push(ConversationFragment::User(latest_user));
                                agent
                                    .conversation_history
                                    .push(ConversationFragment::Assistant(message));
                            }
                        }
                        let mut tx = doc.transaction();
                        reconcile(&mut tx, &agent).unwrap();
                        tx.commit();
                    });
                } else {
                    let response_str =
                        call_inference(main_task_client.as_ref(), req_str.clone(), model_hint.clone())
                            .await;
                    main_task_doc_handle.with_doc_mut(|doc| {
                        let mut agent: LspAgent = hydrate(doc).unwrap();
                        agent
                            .responses
                            .push(AgentResponse::Inference(response_str.clone()));
                        let mut tx = doc.transaction();
                        reconcile(&mut tx, &agent).unwrap();
                        tx.commit();
                    });
                }
            }
        }
    });

    (doc_handle, main_task)
}

async fn call_inference(
    client: &dyn AgentClient,
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
    if let Some(active) = &active_document {
        if !open_documents.contains(active) {
            open_documents.push(active.clone());
        }
    }
    prompts::DocsInfo {
        open_documents,
        active_document,
    }
}
