use agent::start_web_backend;
use async_trait::async_trait;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::thread;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Window, WindowId};
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Mutex};
use traits::{Web, WebAgent};
use wry::{http, RequestAsyncResponder, WebView};

use serde::Deserialize;

#[derive(Debug)]
enum AgentEvent {
    WebApp { id: String, content: String },
    StorageUpdated(String),
}

#[derive(Debug)]
enum BackendCommand {
    CloseApp(String),
}

#[derive(Deserialize)]
struct StoreValueBody {
    key: String,
    value: String,
    description: String,
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
    StoreValue {
        key: String,
        value: String,
        description: String,
        responder: RequestAsyncResponder,
    },
    ReadValue {
        key: String,
        responder: RequestAsyncResponder,
    },
}

struct WebRuntime {
    proxy: tao::event_loop::EventLoopProxy<AgentEvent>,
    pending_inference_requests: Mutex<HashMap<String, VecDeque<RequestAsyncResponder>>>,
}

impl WebRuntime {
    fn new(proxy: tao::event_loop::EventLoopProxy<AgentEvent>) -> Self {
        Self {
            proxy,
            pending_inference_requests: Mutex::new(HashMap::new()),
        }
    }

    async fn enqueue_inference_request(&self, app_id: String, responder: RequestAsyncResponder) {
        let mut pending = self.pending_inference_requests.lock().await;
        pending.entry(app_id).or_default().push_back(responder);
    }

    async fn notify_storage_update(&self, key: String) {
        let _ = self.proxy.send_event(AgentEvent::StorageUpdated(key));
    }
}

#[async_trait]
impl Web for WebRuntime {
    async fn launch_app(&self, id: String, content: String) {
        let _ = self.proxy.send_event(AgentEvent::WebApp { id, content });
    }

    async fn handle_inference_response(&self, app_id: String, content: String) {
        let mut pending = self.pending_inference_requests.lock().await;
        if let Some(queue) = pending.get_mut(&app_id) {
            if let Some(responder) = queue.pop_front() {
                responder.respond(
                    http::Response::builder()
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Vec::from(content))
                        .unwrap(),
                );
                if queue.is_empty() {
                    pending.remove(&app_id);
                }
            } else {
                eprintln!("Received Inference response but no pending responder!");
            }
        } else {
            eprintln!("Received Inference response for unknown app id: {}", app_id);
        }
    }
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

    let web_runtime = Arc::new(WebRuntime::new(proxy));
    let (agent, mut exit_rx) = start_web_backend(web_runtime.clone()).await;

    loop {
        tokio::select! {
            Some(cmd) = backend_rx.recv() => {
                    handle_backend_command(agent.as_ref(), cmd).await;
            }
            Some(req) = api_rx.recv() => {
                    handle_api_request(agent.as_ref(), req, web_runtime.as_ref()).await;
            }
            _ = exit_rx.recv() => {
                break;
            }
        }
    }
}

async fn handle_backend_command(agent: &dyn WebAgent, cmd: BackendCommand) {
    let BackendCommand::CloseApp(app_id) = cmd;
    agent.close_app(app_id).await;
}

async fn handle_api_request(agent: &dyn WebAgent, req: ApiRequest, web_runtime: &WebRuntime) {
    match req {
        ApiRequest::Inference {
            content,
            app_id,
            responder,
        } => {
            let app_id_for_queue = app_id.clone();
            agent.app_inference_request(content, app_id).await;
            web_runtime
                .enqueue_inference_request(app_id_for_queue, responder)
                .await;
        }
        ApiRequest::ReadDocument { uri, responder } => {
            let content = agent.read_document(uri).await;
            responder.respond(
                http::Response::builder()
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Vec::from(content))
                    .unwrap(),
            );
        }
        ApiRequest::StoreValue {
            key,
            value,
            description,
            responder,
        } => {
            agent.store_value(key.clone(), value, description).await;
            web_runtime.notify_storage_update(key).await;
            responder.respond(
                http::Response::builder()
                    .header("Access-Control-Allow-Origin", "*")
                    .status(200)
                    .body(Vec::new())
                    .unwrap(),
            );
        }
        ApiRequest::ReadValue { key, responder } => {
            let value = agent.read_value(key).await;
            let body = value.unwrap_or_default();
            responder.respond(
                http::Response::builder()
                    .header("Access-Control-Allow-Origin", "*")
                    .body(Vec::from(body))
                    .unwrap(),
            );
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
                            } else if uri.to_string().contains("store_value") {
                                let body_str = String::from_utf8_lossy(&body).to_string();
                                match serde_json::from_str::<StoreValueBody>(&body_str) {
                                    Ok(parsed) => {
                                        if let Err(e) =
                                            api_tx.blocking_send(ApiRequest::StoreValue {
                                                key: parsed.key,
                                                value: parsed.value,
                                                description: parsed.description,
                                                responder,
                                            })
                                        {
                                            eprintln!(
                                                "[Web] Failed to send store_value request: {}",
                                                e
                                            );
                                            if let ApiRequest::StoreValue { responder, .. } = e.0 {
                                                responder.respond(
                                                    http::Response::builder()
                                                        .status(500)
                                                        .body(Vec::new())
                                                        .unwrap(),
                                                );
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        eprintln!("[Web] Failed to parse store_value body: {}", e);
                                        responder.respond(
                                            http::Response::builder()
                                                .status(400)
                                                .body(Vec::new())
                                                .unwrap(),
                                        );
                                    }
                                }
                            } else if uri.to_string().contains("read_value") {
                                let key = String::from_utf8_lossy(&body).to_string();
                                if let Err(e) =
                                    api_tx.blocking_send(ApiRequest::ReadValue { key, responder })
                                {
                                    eprintln!("[Web] Failed to send read_value request: {}", e);
                                    if let ApiRequest::ReadValue { responder, .. } = e.0 {
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
            Event::UserEvent(AgentEvent::StorageUpdated(key)) => {
                println!("Storage updated: {}", key);
                if let Ok(safe_key) = serde_json::to_string(&key) {
                    let js = format!(
                        "window.dispatchEvent(new CustomEvent('doc_changed', {{ detail: {{ key: {} }} }}));",
                        safe_key
                    );
                    for (_, webview, _) in views.values() {
                        let _ = webview.evaluate_script(&js);
                    }
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
