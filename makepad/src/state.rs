use std::collections::{HashMap, VecDeque};
use std::sync::{LazyLock, OnceLock, RwLock};

use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
pub enum HostCommand {
    Inference { app_id: String, content: String },
    CloseApp(String),
}

pub struct PendingLaunch {
    pub id: String,
    pub content: String,
}

pub struct AppState {
    pub content: String,
    pub last_request: Option<String>,
    pub last_response: Option<String>,
    pub request_in_flight: bool,
    pub pending_inference: VecDeque<oneshot::Sender<String>>,
}

impl AppState {
    pub fn new(content: String) -> Self {
        Self {
            content,
            last_request: None,
            last_response: None,
            request_in_flight: false,
            pending_inference: VecDeque::new(),
        }
    }
}

pub struct HostState {
    pub revision: u64,
    pub pending_launches: Vec<PendingLaunch>,
    pub app_order: Vec<String>,
    pub apps: HashMap<String, AppState>,
}

impl HostState {
    pub fn bump_revision(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
}

pub static HOST_STATE: LazyLock<RwLock<HostState>> = LazyLock::new(|| {
    RwLock::new(HostState {
        revision: 0,
        pending_launches: Vec::new(),
        app_order: Vec::new(),
        apps: HashMap::new(),
    })
});

pub static COMMAND_TX: OnceLock<mpsc::UnboundedSender<HostCommand>> = OnceLock::new();

pub fn send_command(command: HostCommand) -> Result<(), String> {
    let tx = COMMAND_TX
        .get()
        .ok_or_else(|| "Makepad backend is not ready yet.".to_string())?;

    tx.send(command)
        .map_err(|_| "Makepad backend command channel is closed.".to_string())
}
