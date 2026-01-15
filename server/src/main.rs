use tower_lsp::jsonrpc::Result;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};

struct CustomRequest;

impl tower_lsp::lsp_types::request::Request for CustomRequest {
    type Params = serde_json::Value;
    type Result = serde_json::Value;
    const METHOD: &'static str = "custom/hello";
}

#[derive(Debug)]
struct Backend {
    client: Client,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        self.client.log_message(MessageType::INFO, "Server initializing...").await;
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                ..Default::default()
            },
            ..Default::default()
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        eprintln!("DEBUG: Server initialized method called");
        self.client
            .show_message(MessageType::INFO, "Server initialized!")
            .await;
        self.client.log_message(MessageType::INFO, "Server initialized handler called").await;
            
        // Send custom request after a short delay to ensure client is ready
        let client = self.client.clone();
        tokio::spawn(async move {
            eprintln!("DEBUG: Spawned task starting wait");
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            eprintln!("DEBUG: Sending custom request...");
            client.log_message(MessageType::INFO, "Sending custom request...").await;
            let params = serde_json::json!({ "text": "Hello World from Rust Server!" });
            match client.send_request::<CustomRequest>(params).await {
                Ok(response) => {
                    client.log_message(MessageType::INFO, format!("Full Client Response: {:?}", response)).await;
                    
                    if let Some(text) = response.get("inferenceResult").and_then(|t| t.as_str()) {
                         let model = response.get("modelUsed").and_then(|m| m.as_str()).unwrap_or("unknown");
                         let msg = format!("LLM Inference ({}): {}", model, text);
                         client.show_message(MessageType::INFO, msg).await;
                    } else if let Some(models) = response.get("models").and_then(|m| m.as_array()) {
                        let model_names: Vec<&str> = models.iter()
                            .filter_map(|m| m.get("name").and_then(|n| n.as_str()))
                            .collect();
                        
                        let msg = format!("Loop complete! Client found {} models: {:?}", models.len(), model_names);
                        client.show_message(MessageType::INFO, msg).await;
                    } else {
                        client.show_message(MessageType::WARNING, "Client responded, but no 'models' field found.").await;
                    }
                }
                Err(e) => {
                    let err_msg = format!("Failed to call client: {:?}", e);
                    client.log_message(MessageType::ERROR, &err_msg).await;
                    client.show_message(MessageType::ERROR, err_msg).await;
                }
            }
        });
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| Backend { client });
    Server::new(stdin, stdout, socket).serve(service).await;
}
