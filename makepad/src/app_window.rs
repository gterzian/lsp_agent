use makepad_widgets::*;
use tokio::sync::oneshot;

use crate::state::{send_command, AppState, HostCommand, HOST_STATE};

live_design! {
    use link::theme::*;
    use link::widgets::*;

    pub AppWindowCard = {{AppWindowCard}} {
        visible: false,
        width: Fill,
        height: Fit,
        flow: Down,
        spacing: 10,
        margin: {top: 12, bottom: 0}
        padding: {left: 12, top: 12, right: 12, bottom: 12}
        show_bg: true,
        draw_bg: {
            color: (THEME_COLOR_BG_HIGHLIGHT)
        }

        header = <View> {
            width: Fill,
            height: Fit,
            flow: Right,
            spacing: 10,
            align: {x: 0.0, y: 0.5}

            title = <Label> {
                text: "App"
                draw_text: {
                    text_style: <THEME_FONT_BOLD> {
                        font_size: 18
                    }
                }
            }

            status = <Label> {
                text: "Idle"
            }

            close_button = <Button> {
                text: "Close"
            }
        }

        <Label> { text: "Source" }
        source = <TextInput> {
            height: 140,
            is_read_only: true,
            empty_text: "No app content yet"
        }

        <Label> { text: "Prompt" }
        prompt_input = <TextInput> {
            height: 90,
            empty_text: "Ask this app for inference..."
        }

        send_button = <Button> {
            text: "Send Inference"
        }

        <Label> { text: "Last Request" }
        last_request = <TextInput> {
            height: 80,
            is_read_only: true,
            empty_text: "No request yet"
        }

        <Label> { text: "Last Response" }
        last_response = <TextInput> {
            height: 120,
            is_read_only: true,
            empty_text: "No response yet"
        }
    }
}

#[derive(Clone)]
pub struct AppWindowSnapshot {
    pub id: String,
    pub content: String,
    pub last_request: Option<String>,
    pub last_response: Option<String>,
    pub request_in_flight: bool,
}

impl AppWindowSnapshot {
    pub fn from_state(id: &str, state: &AppState) -> Self {
        Self {
            id: id.to_string(),
            content: state.content.clone(),
            last_request: state.last_request.clone(),
            last_response: state.last_response.clone(),
            request_in_flight: state.request_in_flight,
        }
    }
}

#[derive(Live, LiveHook, Widget)]
pub struct AppWindowCard {
    #[deref]
    view: View,
    #[rust]
    app_id: String,
    #[rust]
    pending_response: Option<oneshot::Receiver<String>>,
}

impl AppWindowCard {
    fn set_status(&mut self, cx: &mut Cx, text: &str) {
        self.label(id!(status)).set_text(cx, text);
    }

    fn set_snapshot_internal(&mut self, cx: &mut Cx, snapshot: Option<&AppWindowSnapshot>) {
        match snapshot {
            Some(snapshot) => {
                if self.app_id != snapshot.id {
                    self.pending_response = None;
                }

                self.app_id = snapshot.id.clone();
                self.view.set_visible(cx, true);
                self.label(id!(title))
                    .set_text(cx, &format!("App {}", snapshot.id));
                self.set_status(
                    cx,
                    if snapshot.request_in_flight {
                        "Waiting for inference response"
                    } else {
                        "Ready"
                    },
                );
                self.widget(id!(source)).set_text(cx, &snapshot.content);
                self.widget(id!(last_request))
                    .set_text(cx, snapshot.last_request.as_deref().unwrap_or_default());
                self.widget(id!(last_response))
                    .set_text(cx, snapshot.last_response.as_deref().unwrap_or_default());
            }
            None => {
                self.app_id.clear();
                self.pending_response = None;
                self.label(id!(title)).set_text(cx, "");
                self.set_status(cx, "");
                self.widget(id!(source)).set_text(cx, "");
                self.widget(id!(prompt_input)).set_text(cx, "");
                self.widget(id!(last_request)).set_text(cx, "");
                self.widget(id!(last_response)).set_text(cx, "");
                self.view.set_visible(cx, false);
            }
        }
    }

    fn refresh_from_state(&mut self, cx: &mut Cx) {
        if self.app_id.is_empty() {
            return;
        }

        let snapshot = {
            let state = HOST_STATE.read().unwrap();
            state
                .apps
                .get(&self.app_id)
                .map(|app| AppWindowSnapshot::from_state(&self.app_id, app))
        };

        self.set_snapshot_internal(cx, snapshot.as_ref());
    }

    fn start_inference(&mut self, cx: &mut Cx, prompt: String) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() || self.app_id.is_empty() {
            return;
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut state = HOST_STATE.write().unwrap();
            let Some(app) = state.apps.get_mut(&self.app_id) else {
                self.set_status(cx, "App state is unavailable.");
                return;
            };

            app.last_request = Some(prompt.clone());
            app.request_in_flight = true;
            app.pending_inference.push_back(tx);
        }

        if let Err(error) = send_command(HostCommand::Inference {
            app_id: self.app_id.clone(),
            content: prompt,
        }) {
            let mut state = HOST_STATE.write().unwrap();
            if let Some(app) = state.apps.get_mut(&self.app_id) {
                app.request_in_flight = false;
                let _ = app.pending_inference.pop_back();
                app.last_response = Some(error.clone());
            }
            drop(state);
            self.set_status(cx, &error);
            self.widget(id!(last_response)).set_text(cx, &error);
            return;
        }

        self.pending_response = Some(rx);
        self.widget(id!(prompt_input)).set_text(cx, "");
        self.refresh_from_state(cx);
    }

    fn close_app(&mut self, cx: &mut Cx) {
        if self.app_id.is_empty() {
            return;
        }

        if let Err(error) = send_command(HostCommand::CloseApp(self.app_id.clone())) {
            self.set_status(cx, &error);
            return;
        }

        let mut state = HOST_STATE.write().unwrap();
        state.apps.remove(&self.app_id);
        state.app_order.retain(|id| id != &self.app_id);
        drop(state);

        self.pending_response = None;
        self.set_snapshot_internal(cx, None);
    }

    fn poll_response(&mut self, cx: &mut Cx) {
        let Some(rx) = &mut self.pending_response else {
            return;
        };

        match rx.try_recv() {
            Ok(_) => {
                self.pending_response = None;
                self.refresh_from_state(cx);
            }
            Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                self.pending_response = None;
                self.set_status(cx, "Inference response channel closed.");
            }
        }
    }
}

impl Widget for AppWindowCard {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event, scope: &mut Scope) {
        let actions = cx.capture_actions(|cx| {
            self.view.handle_event(cx, event, scope);
        });

        self.poll_response(cx);
        self.refresh_from_state(cx);

        if let Some((prompt, _)) = self.text_input(id!(prompt_input)).returned(&actions) {
            self.start_inference(cx, prompt);
        }

        if self.button(id!(send_button)).clicked(&actions) {
            let prompt = self.widget(id!(prompt_input)).text();
            self.start_inference(cx, prompt);
        }

        if self.button(id!(close_button)).clicked(&actions) {
            self.close_app(cx);
        }

        cx.extend_actions(actions);
    }

    fn draw_walk(&mut self, cx: &mut Cx2d, scope: &mut Scope, walk: Walk) -> DrawStep {
        self.view.draw_walk(cx, scope, walk)
    }
}

impl AppWindowCardRef {
    pub fn set_snapshot(&self, cx: &mut Cx, snapshot: Option<&AppWindowSnapshot>) {
        if let Some(mut inner) = self.borrow_mut() {
            inner.set_snapshot_internal(cx, snapshot);
        }
    }
}
