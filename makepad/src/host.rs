use async_trait::async_trait;
use makepad_widgets::SignalToUI;
use traits::App;

use crate::state::{PendingLaunch, HOST_STATE};

fn log_preview(content: &str) -> String {
    let mut preview: String = content.chars().take(120).collect();
    preview = preview.replace('\n', "\\n");
    if content.chars().count() > 120 {
        preview.push_str("...");
    }
    preview
}

pub struct MakepadAppHost {
    pub signal: SignalToUI,
}

#[async_trait]
impl App for MakepadAppHost {
    async fn launch_app(&self, id: String, content: String) {
        let mut state = HOST_STATE.write().unwrap();
        let pending_before = state.pending_launches.len();
        eprintln!(
            "[LSP Agent Host] Queueing launch {} (pending before: {}, {} chars): {}",
            id,
            pending_before,
            content.chars().count(),
            log_preview(&content)
        );
        state.pending_launches.push(PendingLaunch { id, content });
        state.bump_revision();
        eprintln!(
            "[LSP Agent Host] Pending launches after queue: {}, revision={}",
            state.pending_launches.len(),
            state.revision
        );
        drop(state);
        self.signal.set();
    }

    async fn handle_inference_response(&self, app_id: String, content: String) {
        let mut state = HOST_STATE.write().unwrap();
        if let Some(app) = state.apps.get_mut(&app_id) {
            app.last_response = Some(content.clone());
            app.request_in_flight = false;

            if let Some(tx) = app.pending_inference.pop_front() {
                let _ = tx.send(content);
            }
            state.bump_revision();
        }
        drop(state);
        self.signal.set();
    }
}
