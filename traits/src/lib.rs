use async_trait::async_trait;

#[async_trait]
pub trait AgentClient: Send + Sync {
    async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
    async fn notify_shutdown(&self);
}

#[async_trait]
pub trait Agent: Send + Sync {
    async fn shutdown(&self);
    async fn did_open(&self, uri: String, text: String);
    async fn did_change(&self, uri: String, text: String);
    async fn did_close(&self, uri: String);
    async fn set_active_document(&self, uri: String);
    async fn chat_request(&self, content: String, model: Option<String>);
}
