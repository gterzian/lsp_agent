use async_trait::async_trait;
use makepad_widgets::SignalToUI;
use traits::App;

use crate::state::{PendingLaunch, HOST_STATE};

pub struct MakepadAppHost {
    pub signal: SignalToUI,
}

#[async_trait]
impl App for MakepadAppHost {
    async fn launch_app(&self, id: String, content: String) {
        let mut state = HOST_STATE.write().unwrap();
        state.pending_launches.push(PendingLaunch { id, content });
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
        }
        drop(state);
        self.signal.set();
    }
}
