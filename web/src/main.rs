use automerge_repo::{ConnDirection, DocumentId, Repo, Storage, StorageError};
use autosurgeon::{hydrate, reconcile};
use axum::{routing::get, Router};
use futures::future::BoxFuture;
use shared_document::{ChatRequest, Id, LspAgent, NoStorage};
use std::future::IntoFuture;
use tokio::net::TcpListener;
use tokio::runtime::Handle;
use tokio::sync::oneshot;
use tokio::time::{sleep, Duration};
use uuid::Uuid;

const PEER2_PORT: u16 = 2342;

#[tokio::main]
async fn main() {
    let handle = Handle::current();
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

    // Wait for sync (simple hacky wait)
    sleep(Duration::from_millis(2000)).await;

    let req_id = Id {
        value: Uuid::new_v4().to_string(),
    };

    println!("Sending Request...");
    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = hydrate(doc).unwrap();
        agent.requests.insert(
            req_id.clone(),
            ChatRequest {
                content: "Hello from Web!".to_string(),
            },
        );
        let mut tx = doc.transaction();
        reconcile(&mut tx, &agent).unwrap();
        tx.commit();
    });

    // Spawn a task to watch for response
    let doc_handle_clone = doc_handle.clone();
    let req_id_clone = req_id.clone();
    handle.spawn(async move {
        loop {
            doc_handle_clone.changed().await.unwrap();
            let agent: LspAgent = doc_handle_clone.with_doc(|doc| hydrate(doc).unwrap());
            if let Some(resp) = agent.responses.get(&req_id_clone) {
                println!("Received Response: {}", resp.content);
                break;
            }
        }
    });

    let app = Router::new().route("/health", get(|| async { "ok" }));

    let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
    println!("Web Client listening on 0.0.0.0:3000");

    let serve = axum::serve(listener, app);

    tokio::select! {
        _ = serve.into_future() => {},
        _ = tokio::signal::ctrl_c() => {
            println!("Shutting down...");
            handle.spawn_blocking(move || {
                repo_handle.stop().unwrap();
            }).await.unwrap();
        }
    }
}
