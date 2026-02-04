use async_trait::async_trait;

#[async_trait]
pub trait InferenceClient: Send + Sync {
    async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
    async fn notify_shutdown(&self);
}

#[async_trait]
pub trait WorkspaceAgent: Send + Sync {
    async fn shutdown(&self);
    async fn did_open(&self, uri: String, text: String);
    async fn did_change(&self, uri: String, text: String);
    async fn did_close(&self, uri: String);
    async fn set_active_document(&self, uri: String);
    async fn chat_request(&self, content: String, model: Option<String>) -> Option<String>;
}

#[async_trait]
pub trait WebAgent: Send + Sync {
    async fn app_inference_request(&self, content: String, app_id: String);
    async fn read_document(&self, uri: String) -> String;
    async fn close_app(&self, app_id: String);
}

#[async_trait]
pub trait Web: Send + Sync {
    async fn launch_app(&self, id: String, content: String);
    async fn handle_inference_response(&self, app_id: String, content: String);
}
