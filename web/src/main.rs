use automerge_repo::{ConnDirection, DocumentId, Repo, Storage, StorageError};
use autosurgeon::{hydrate, reconcile};
use shared_document::{ChatRequest, Id, LspAgent, NoStorage};
use std::thread;
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use uuid::Uuid;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::window::{Window, WindowAttributes, WindowId};
use wry::WebView;

const PEER2_PORT: u16 = 2342;

#[derive(Debug)]
enum AgentEvent {
    Response(String),
    ShutDown,
}

#[derive(Debug)]
enum BackendCommand {
    Start,
    Shutdown,
}

struct App {
    window: Option<Window>,
    webview: Option<WebView>,
    tx_backend: mpsc::Sender<BackendCommand>,
}

impl ApplicationHandler<AgentEvent> for App {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {
        // Send signal to start the backend logic
        if self.window.is_none() {
            let _ = self.tx_backend.blocking_send(BackendCommand::Start);
        }
    }

    fn window_event(&mut self, _event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                println!("The close button was pressed; sending shutdown signal");
                let _ = self.tx_backend.blocking_send(BackendCommand::Shutdown);
            }
            _ => (),
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AgentEvent) {
        match event {
            AgentEvent::Response(html) => {
                println!("Received HTML response, creating webview...");
                if self.window.is_none() {
                    let window = event_loop
                        .create_window(WindowAttributes::default().with_title("LSP Agent Web"))
                        .unwrap();
                    self.window = Some(window);
                }

                if self.webview.is_none() {
                    if let Some(window) = &self.window {
                        let webview = wry::WebViewBuilder::new()
                            .with_html(html)
                            .build(window)
                            .unwrap();
                        self.webview = Some(webview);
                    }
                }
            }
            AgentEvent::ShutDown => {
                println!("Shutting down due to signal...");
                event_loop.exit();
            }
        }
    }
}

fn main() {
    let event_loop = EventLoop::<AgentEvent>::with_user_event().build().unwrap();
    let proxy = event_loop.create_proxy();

    let (tx_backend, mut rx_backend) = mpsc::channel(10);

    thread::spawn(move || {
        let rt = Runtime::new().unwrap();
        rt.block_on(async move {
            println!("Backend thread started...");

            let repo = Repo::new(None, Box::new(NoStorage));
            let repo_handle = repo.run();
            let repo_clone = repo_handle.clone();
            let addr = format!("127.0.0.1:{}", PEER2_PORT);

            // Listen as Peer 2
            tokio::spawn(async move {
                match TcpListener::bind(&addr).await {
                    Ok(listener) => loop {
                        if let Ok((socket, addr)) = listener.accept().await {
                            repo_clone
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

            let doc_id: DocumentId = doc_id_str.parse().expect("Failed to parse document ID");
            println!("Found Doc ID: {}", doc_id);

            // Request Doc
            let doc_handle = repo_handle.request_document(doc_id.clone()).await.unwrap();
            let mut req_id: Option<Id> = None;

            loop {
                tokio::select! {
                    _ = doc_handle.changed() => {
                        let (should_exit, response_content) = doc_handle.with_doc(|doc| {
                            let agent: LspAgent = hydrate(doc).unwrap();
                            let resp =  if let Some(rid) = &req_id {
                                agent.responses.get(rid).map(|r| r.content.clone())
                            } else {
                                None
                            };
                            (agent.should_exit, resp)
                        });

                        if should_exit {
                            let _ = proxy.send_event(AgentEvent::ShutDown);
                            break;
                        }

                        if let Some(content) = response_content {
                            println!("Received Response in backend!");
                            let _ = proxy.send_event(AgentEvent::Response(content));
                        }
                    }
                    Some(cmd) = rx_backend.recv() => {
                        match cmd {
                            BackendCommand::Start => {
                                if req_id.is_some() { continue; }
                                println!("Sending Request...");
                                let rid = Id {
                                    value: Uuid::new_v4().to_string(),
                                };
                                let system_prompt = include_str!("../../prompts/web-environment.md");
                                let request_content =
                                    format!("{}\n\n{}", system_prompt, "Log Hello world to the console");
                                doc_handle.with_doc_mut(|doc| {
                                    let mut agent: LspAgent = hydrate(doc).unwrap();
                                    agent.requests.insert(
                                        rid.clone(),
                                        ChatRequest {
                                            content: request_content,
                                        },
                                    );
                                    let mut tx = doc.transaction();
                                    reconcile(&mut tx, &agent).unwrap();
                                    tx.commit();
                                });
                                req_id = Some(rid);
                            }
                            BackendCommand::Shutdown => {
                                println!("Backend received shutdown command, updating doc...");
                                doc_handle.with_doc_mut(|doc| {
                                    let mut agent: LspAgent = hydrate(doc).unwrap();
                                    agent.should_exit = true;
                                    let mut tx = doc.transaction();
                                    reconcile(&mut tx, &agent).unwrap();
                                    tx.commit();
                                });
                            }
                        }
                    }
                }
            }
        });
    });

    let mut app = App {
        window: None,
        webview: None,
        tx_backend: tx_backend,
    };

    event_loop.run_app(&mut app).unwrap();
}
