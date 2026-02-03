use automerge_repo::{ConnDirection, DocumentId, Repo};
use autosurgeon::{hydrate, reconcile};
use shared_document::{LspAgent, NoStorage};
use std::collections::HashMap;
use std::thread;
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder};
use tao::window::{Window, WindowId};
use tokio::net::TcpListener;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};
use wry::WebView;

const PEER2_PORT: u16 = 2342;

#[derive(Debug)]
enum AgentEvent {
    Response(String),
}

fn main() {
    let event_loop = EventLoopBuilder::<AgentEvent>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    let backend_handle = thread::spawn(move || {
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

            loop {
                tokio::select! {
                    _ = doc_handle.changed() => {
                        let (should_exit, response_content) = doc_handle.with_doc_mut(|doc| {
                            let mut agent: LspAgent = hydrate(doc).unwrap();
                            let resp = if !agent.responses.is_empty() {
                                Some(agent.responses.remove(0).content)
                            } else {
                                None
                            };

                            if resp.is_some() {
                                let mut tx = doc.transaction();
                                reconcile(&mut tx, &agent).unwrap();
                                tx.commit();
                            }

                            (agent.should_exit, resp)
                        });

                        if should_exit {
                            break;
                        }

                        if let Some(content) = response_content {
                            println!("Received Response in backend!");
                            let _ = proxy.send_event(AgentEvent::Response(content));
                        }
                    }
                }
            }
        });
    });

    let mut views: HashMap<WindowId, (Window, WebView)> = HashMap::new();
    let mut backend_handle_opt = Some(backend_handle);

    event_loop.run(move |event, window_target, control_flow| {
        *control_flow = ControlFlow::Wait;

        match event {
            Event::UserEvent(AgentEvent::Response(content)) => {
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

                let webview = wry::WebViewBuilder::new()
                    .with_html(clean_content)
                    .build(&window)
                    .unwrap();

                views.insert(id, (window, webview));
            }
            Event::WindowEvent {
                event: WindowEvent::CloseRequested,
                window_id,
                ..
            } => {
                println!("The close button was pressed.");
                views.remove(&window_id);
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
