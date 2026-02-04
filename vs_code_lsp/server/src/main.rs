use agent::start_infra;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tower_lsp::jsonrpc::Result as LspResult;
use tower_lsp::lsp_types::*;
use tower_lsp::{Client, LanguageServer, LspService, Server};
use traits::{Agent, InferenceClient};

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

enum ShutdownExtension {}

impl tower_lsp::lsp_types::notification::Notification for ShutdownExtension {
    type Params = ();
    const METHOD: &'static str = "lsp-agent/shutdown";
}

struct LspAgentClient {
    client: Client,
}

#[async_trait::async_trait]
impl InferenceClient for LspAgentClient {
    async fn inference(
        &self,
        request: String,
        model: Option<String>,
    ) -> std::result::Result<String, String> {
        let params = InferenceParams { request, model };
        match self
            .client
            .send_request::<InferenceLspRequest>(params)
            .await
        {
            Ok(res) => Ok(res.response),
            Err(e) => Err(format!("{:?}", e)),
        }
    }

    async fn notify_shutdown(&self) {
        let _ = self.client.send_notification::<ShutdownExtension>(()).await;
    }
}

struct Backend {
    client: Client,
    agent: Box<dyn Agent>,
}

#[tower_lsp::async_trait]
impl LanguageServer for Backend {
    async fn initialize(&self, _: InitializeParams) -> LspResult<InitializeResult> {
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

    async fn shutdown(&self) -> LspResult<()> {
        self.agent.shutdown().await;
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        let text = params.text_document.text;
        self.agent.did_open(uri, text).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        if let Some(change) = params.content_changes.last() {
            self.agent.did_change(uri, change.text.clone()).await;
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        let uri = params.text_document.uri.to_string();
        self.agent.did_close(uri).await;
    }

    async fn execute_command(
        &self,
        params: ExecuteCommandParams,
    ) -> LspResult<Option<serde_json::Value>> {
        if params.command == "lsp-agent.log-chat" {
            if let Some(arg) = params.arguments.first().and_then(|v| v.as_str()) {
                let user_input = arg.to_string();
                let model = params
                    .arguments
                    .get(1)
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string());
                let response = self.agent.chat_request(user_input, model).await;
                if let Some(message) = response {
                    return Ok(Some(serde_json::Value::String(message)));
                }
                return Ok(None);
            }
        } else if params.command == "lsp-agent.active-doc" {
            if let Some(uri) = params.arguments.first().and_then(|v| v.as_str()) {
                self.agent.set_active_document(uri.to_string()).await;
            }
        }
        Ok(None)
    }
}

#[tokio::main]
async fn main() {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(|client| {
        let agent_client = Arc::new(LspAgentClient {
            client: client.clone(),
        });
        let agent = start_infra(agent_client);
        Backend { client, agent }
    });
    Server::new(stdin, stdout, socket).serve(service).await;
}
