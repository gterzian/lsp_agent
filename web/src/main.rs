use agent::{AgentRequest, AgentResponse, ConversationFragment, LspAgent, NoStorage};
use automerge_repo::{ConnDirection, DocumentId, Repo};
use autosurgeon::{hydrate, reconcile};
use std::collections::{HashMap, VecDeque};
use std::thread;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Window, WindowId};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use wry::{http, RequestAsyncResponder, WebView};

const PEER2_PORT: u16 = 2342;

#[derive(Debug)]
enum AgentEvent {
    WebApp { id: String, content: String },
}

#[derive(Debug)]
enum BackendCommand {
    CloseApp(String),
}

enum ApiRequest {
    Inference {
        content: String,
        app_id: String,
        responder: RequestAsyncResponder,
    },
    ReadDocument {
        uri: String,
        responder: RequestAsyncResponder,
    },
}

fn spawn_backend_thread(
    api_rx: mpsc::Receiver<ApiRequest>,
    backend_rx: mpsc::Receiver<BackendCommand>,
    proxy: tao::event_loop::EventLoopProxy<AgentEvent>,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(run_backend(api_rx, backend_rx, proxy));
    })
}

async fn run_backend(
    mut api_rx: mpsc::Receiver<ApiRequest>,
    mut backend_rx: mpsc::Receiver<BackendCommand>,
    proxy: tao::event_loop::EventLoopProxy<AgentEvent>,
) {
    println!("Backend thread started...");

    let repo = Repo::new(None, Box::new(NoStorage));
    let repo_handle = repo.run();
    listen_peer2(repo_handle.clone());

    let doc_id = wait_for_doc_id().await;
    println!("Found Doc ID: {}", doc_id);

    let doc_handle = repo_handle.request_document(doc_id.clone()).await.unwrap();
    let mut pending_inference_requests: HashMap<String, VecDeque<RequestAsyncResponder>> =
        HashMap::new();

    loop {
        tokio::select! {
            Some(cmd) = backend_rx.recv() => {
                handle_backend_command(&doc_handle, cmd);
            }
            Some(req) = api_rx.recv() => {
                handle_api_request(&doc_handle, req, &mut pending_inference_requests);
            }
            _ = doc_handle.changed() => {
                if handle_doc_change(&doc_handle, &proxy, &mut pending_inference_requests) {
                    break;
                }
            }
        }
    }
}

fn listen_peer2(repo_handle: automerge_repo::RepoHandle) {
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

fn handle_backend_command(doc_handle: &automerge_repo::DocHandle, cmd: BackendCommand) {
    let BackendCommand::CloseApp(app_id) = cmd;
    doc_handle.with_doc_mut(|doc| {
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

fn handle_api_request(
    doc_handle: &automerge_repo::DocHandle,
    req: ApiRequest,
    pending_inference_requests: &mut HashMap<String, VecDeque<RequestAsyncResponder>>,
) {
    match req {
        ApiRequest::Inference {
            content,
            app_id,
            responder,
        } => {
            let app_id_for_queue = app_id.clone();
            doc_handle.with_doc_mut(|doc| {
                let mut agent: LspAgent = hydrate(doc).unwrap();
                agent
                    .requests
                    .push(AgentRequest::Inference { content, app_id });
                let mut tx = doc.transaction();
                reconcile(&mut tx, &agent).unwrap();
                tx.commit();
            });
            pending_inference_requests
                .entry(app_id_for_queue)
                .or_default()
                .push_back(responder);
        }
        ApiRequest::ReadDocument { uri, responder } => {
            let content = doc_handle.with_doc_mut(|doc| {
                let agent: LspAgent = hydrate(doc).unwrap();
                agent
                    .text_documents
                    .documents
                    .get(&uri)
                    .map(|doc| doc.text.clone())
                    .unwrap_or_default()
            });
            responder.respond(
                http::Response::builder()
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Vec::from(content))
                    .unwrap(),
            );
        }
    }
}

fn handle_doc_change(
    doc_handle: &automerge_repo::DocHandle,
    proxy: &tao::event_loop::EventLoopProxy<AgentEvent>,
    pending_inference_requests: &mut HashMap<String, VecDeque<RequestAsyncResponder>>,
) -> bool {
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
            handle_response(doc_handle, proxy, pending_inference_requests, resp);
        }
    }

    false
}

fn take_response(doc_handle: &automerge_repo::DocHandle) -> Option<AgentResponse> {
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

fn handle_response(
    _doc_handle: &automerge_repo::DocHandle,
    proxy: &tao::event_loop::EventLoopProxy<AgentEvent>,
    pending_inference_requests: &mut HashMap<String, VecDeque<RequestAsyncResponder>>,
    resp: AgentResponse,
) {
    match resp {
        AgentResponse::WebApp { id, content } => {
            println!("Received WebApp response in backend!");
            let _ = proxy.send_event(AgentEvent::WebApp { id, content });
        }
        AgentResponse::Chat(_) => {
            debug_assert!(false, "Web backend should not consume chat responses");
        }
        AgentResponse::Inference { app_id, content } => {
            if let Some(queue) = pending_inference_requests.get_mut(&app_id) {
                if let Some(responder) = queue.pop_front() {
                    responder.respond(
                        http::Response::builder()
                            .header("Access-Control-Allow-Origin", "*")
                            .body(Vec::from(content))
                            .unwrap(),
                    );
                    if queue.is_empty() {
                        pending_inference_requests.remove(&app_id);
                    }
                } else {
                    eprintln!("Received Inference response but no pending responder!");
                }
            } else {
                eprintln!("Received Inference response for unknown app id: {}", app_id);
            }
        }
    }
}

fn main() {
    let event_loop = EventLoopBuilder::<AgentEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let (api_tx, api_rx) = mpsc::channel::<ApiRequest>(32);
    let (backend_tx, backend_rx) = mpsc::channel::<BackendCommand>(32);

    let backend_handle = spawn_backend_thread(api_rx, backend_rx, proxy);

    let mut views: HashMap<WindowId, (Window, WebView, String)> = HashMap::new();
    let mut backend_handle_opt = Some(backend_handle);
    let api_tx = api_tx.clone();
    let backend_tx = backend_tx.clone();

    event_loop.run(move |event, window_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(AgentEvent::WebApp {
                id: app_id,
                content,
            }) => {
                println!("Received HTML response, creating webview...");

                let window = tao::window::WindowBuilder::new()
                    .with_title("LSP Agent Web")
                    .build(window_target)
                    .unwrap();
                let id = window.id();

                let clean_content = content.trim();
                let clean_content = if let Some(stripped) = clean_content.strip_prefix("```html") {
                    stripped
                } else if let Some(stripped) = clean_content.strip_prefix("```") {
                    stripped
                } else {
                    clean_content
                };
                let clean_content = clean_content
                    .strip_suffix("```")
                    .unwrap_or(clean_content)
                    .trim();

                let api_tx = api_tx.clone();
                let app_id_for_requests = app_id.clone();
                let webview = wry::WebViewBuilder::new()
                    .with_asynchronous_custom_protocol(
                        "wry".into(),
                        move |_webview_id, request, responder| {
                            let api_tx = api_tx.clone();
                            let app_id_for_requests = app_id_for_requests.clone();
                            let uri = request.uri().clone();
                            let body = request.body().clone();
                            eprintln!("[Web] Received custom protocol request: {}", uri);
                            if uri.to_string().contains("inference") {
                                let body_str = String::from_utf8_lossy(&body).to_string();
                                eprintln!(
                                    "[Web] Forwarding inference request: {} chars",
                                    body_str.len()
                                );
                                if let Err(e) = api_tx.blocking_send(ApiRequest::Inference {
                                    content: body_str,
                                    app_id: app_id_for_requests,
                                    responder,
                                }) {
                                    eprintln!("[Web] Failed to send API request: {}", e);
                                    if let ApiRequest::Inference { responder, .. } = e.0 {
                                        responder.respond(
                                            http::Response::builder()
                                                .status(500)
                                                .body(Vec::new())
                                                .unwrap(),
                                        );
                                    }
                                }
                            } else if uri.to_string().contains("document") {
                                let body_str = String::from_utf8_lossy(&body).to_string();
                                if let Err(e) = api_tx.blocking_send(ApiRequest::ReadDocument {
                                    uri: body_str,
                                    responder,
                                }) {
                                    eprintln!("[Web] Failed to send document request: {}", e);
                                    if let ApiRequest::ReadDocument { responder, .. } = e.0 {
                                        responder.respond(
                                            http::Response::builder()
                                                .status(500)
                                                .body(Vec::new())
                                                .unwrap(),
                                        );
                                    }
                                }
                            } else {
                                eprintln!("[Web] Unknown URI: {}", uri);
                                responder.respond(
                                    http::Response::builder()
                                        .header("Access-Control-Allow-Origin", "*")
                                        .status(404)
                                        .body(Vec::new())
                                        .unwrap(),
                                );
                            }
                        },
                    )
                    .with_html(clean_content)
                    .build(&window)
                    .unwrap();

                views.insert(id, (window, webview, app_id));
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                println!("The close button was pressed.");
                if let Some((_, _, app_id)) = views.remove(&window_id) {
                    let _ = backend_tx.blocking_send(BackendCommand::CloseApp(app_id));
                }
            }
            Event::LoopDestroyed => {
                if let Some(handle) = backend_handle_opt.take() {
                    let _ = handle.join();
                }
            }
            _ => (),
        }
    });
}
