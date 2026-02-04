use automerge_repo::{ConnDirection, DocHandle, Repo};
use autosurgeon::{hydrate, reconcile};
use serde::{Deserialize, Serialize};
use shared_document::{
    AgentRequest, AgentResponse, ConversationFragment, DocumentContent, DocumentManager, LspAgent,
    NoStorage, Uri,
};
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
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
    model: Option<String>,
}

#[derive(Serialize, Deserialize, Debug)]
struct InferenceResult {
    response: String,
}

#[derive(Deserialize, Debug)]
struct ToolResponse {
    action: String,
    message: Option<String>,
    app: Option<String>,
}

#[derive(Deserialize)]
struct ToolRequest {
    latest_user: String,
}

enum ShutdownExtension {}

impl tower_lsp::lsp_types::notification::Notification for ShutdownExtension {
    type Params = ();
    const METHOD: &'static str = "lsp-agent/shutdown";
}

struct Backend {
    client: Client,
    doc_handle: DocHandle,
    agent_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    web_child: Mutex<Option<Child>>,
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

        if let Some(mut child) = self.web_child.lock().await.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
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
                let user_input = arg.to_string();

                let model = params
                    .arguments
                    .get(1)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());

                let (history, running_apps) = self.doc_handle.with_doc_mut(|doc| {
                    let agent: LspAgent = hydrate(doc).unwrap();
                    (
                        agent.conversation_history.clone(),
                        collect_apps(&agent.webviews),
                    )
                });
                let docs_info = self.doc_handle.with_doc_mut(|doc| {
                    let agent: LspAgent = hydrate(doc).unwrap();
                    collect_docs(&agent.text_documents)
                });

                let mut apps_payload: Option<Vec<String>> = None;
                let mut docs_payload: Option<prompts::DocsInfo> = None;
                let mut response_message: Option<String> = None;
                let mut launched_app: Option<String> = None;

                for _ in 0..3 {
                    let request_text = prompts::build_web_request(
                        &history,
                        &user_input,
                        apps_payload.as_deref(),
                        docs_payload.as_ref(),
                    );
                    let response_str =
                        call_inference(&self.client, request_text, model.clone()).await;
                    let tool_response = parse_tool_response(&response_str);

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
                            response_message = Some(response_str);
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

                if let Some(app) = launched_app {
                    self.doc_handle.with_doc_mut(|doc| {
                        let mut agent: LspAgent = hydrate(doc).unwrap();
                        let app_id = format!("app-{}", Uuid::new_v4());
                        agent
                            .webviews
                            .documents
                            .insert(app_id.clone(), DocumentContent { text: app.clone() });
                        agent.responses.push(AgentResponse::WebApp {
                            id: app_id.clone(),
                            content: app,
                        });
                        if let Some(m) = &model {
                            agent.active_model = Some(m.clone());
                        }
                        let mut tx = doc.transaction();
                        reconcile(&mut tx, &agent).unwrap();
                        tx.commit();
                    });

                    return Ok(None);
                }

                if let Some(message) = response_message.clone() {
                    self.doc_handle.with_doc_mut(|doc| {
                        let mut agent: LspAgent = hydrate(doc).unwrap();
                        agent
                            .conversation_history
                            .push(ConversationFragment::User(user_input.clone()));
                        agent
                            .conversation_history
                            .push(ConversationFragment::Assistant(message.clone()));
                        if let Some(m) = &model {
                            agent.active_model = Some(m.clone());
                        }
                        let mut tx = doc.transaction();
                        reconcile(&mut tx, &agent).unwrap();
                        tx.commit();
                    });

                    return Ok(Some(serde_json::Value::String(message)));
                }
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

async fn call_inference(client: &Client, request: String, model: Option<String>) -> String {
    let params = InferenceParams { request, model };
    match client.send_request::<InferenceLspRequest>(params).await {
        Ok(res) => res.response,
        Err(e) => {
            eprintln!("LSP Inference Error: {:?}", e);
            format!("Error: {:?}", e)
        }
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

fn extract_latest_user(request: &str) -> Option<String> {
    serde_json::from_str::<ToolRequest>(request)
        .ok()
        .map(|req| req.latest_user)
}

const PEER1_PORT: u16 = 2341;
const PEER2_PORT: u16 = 2342;

fn start_automerge_infrastructure(client: Client) -> (DocHandle, tokio::task::JoinHandle<()>) {
    let handle = Handle::current();

    // 1. Setup Server (Repo 1, Port 2341)
    let repo1 = Repo::new(None, Box::new(NoStorage));
    let repo_handle1 = repo1.run();

    // 2. Bootstrap Document
    let doc_handle = repo_handle1.new_document();
    let doc_id = doc_handle.document_id();

    // Spawn HTTP Server for doc_id
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
                main_task_client
                    .send_notification::<ShutdownExtension>(())
                    .await;
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
                        main_task_client
                            .log_message(
                                MessageType::INFO,
                                format!(
                                    "App Inference Request (using active model {:?}, app {}): {}",
                                    active_model, app_id, content
                                ),
                            )
                            .await;
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

                    let latest_user = extract_latest_user(&req_str).unwrap_or_default();
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
                            call_inference(&main_task_client, request_text, model_hint.clone())
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
                        call_inference(&main_task_client, req_str.clone(), model_hint.clone())
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

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let exe_path = std::env::current_exe().expect("Failed to get current exe path");
    // Assuming structure: lsp_agent/server/target/debug/server
    // We want: lsp_agent/web/target/debug/web
    let project_root = exe_path
        .parent() // debug
        .and_then(|p| p.parent()) // target
        .and_then(|p| p.parent()) // server
        .and_then(|p| p.parent()) // lsp_agent
        .expect("Failed to find project root");

    let web_binary = project_root.join("web/target/debug/web");

    let mut child = Command::new(web_binary);

    // Explicitly inherit stderr to see what is happening,
    // but ensure stdout is piped or null so it doesn't break LSP JSON-RPC
    child.stdout(std::process::Stdio::null());
    child.stderr(std::process::Stdio::inherit());

    let child = child.spawn().expect("Failed to spawn web client");

    let (service, socket) = LspService::new(|client| {
        let (doc_handle, task) = start_automerge_infrastructure(client.clone());
        Backend {
            client,
            doc_handle,
            agent_task: Mutex::new(Some(task)),
            web_child: Mutex::new(Some(child)),
        }
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
