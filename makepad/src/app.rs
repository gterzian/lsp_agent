use std::collections::HashMap;

use makepad_widgets::*;
use tokio::sync::oneshot;

use crate::state::{send_command, AppState, HostCommand, HOST_STATE};

const MAX_APP_SLOTS: usize = 8;

app_main!(MakepadRootApp);

script_mod! {
    use mod.prelude.widgets.*
    use mod.widgets.*

    mod.widgets.AgentSplashBase = #(crate::agent_splash::AgentSplash::register_widget(vm))

    mod.widgets.AgentSplash = set_type_default() do mod.widgets.AgentSplashBase{
        width: Fill height: Fit
    }

    let HostHeader = RoundedView{
        width: Fill height: Fit padding: 16 flow: Down spacing: 6
        draw_bg.color: #252836
        title := Label{text: "LSP Agent Makepad Host" draw_text.text_style.font_size: 24 draw_text.color: #fff}
        subtitle := Label{text: "The host stays open when the server launches. Each launched app is rendered as live Splash inside a host card." draw_text.color: #b7bfd3 draw_text.text_style.font_size: 11}
    }

    let EmptyState = RoundedView{
        width: Fill height: Fit padding: 18 flow: Down spacing: 8
        draw_bg.color: #202430
        empty_title := Label{text: "No apps launched yet" draw_text.color: #fff draw_text.text_style.font_size: 16}
        empty_body := Label{text: "Ask the agent to create a Makepad app and the Splash body will appear here immediately." draw_text.color: #a5adc1 draw_text.text_style.font_size: 11}
    }

    let AppSlot = RoundedView{
        visible: false
        width: Fill height: Fit padding: 14 flow: Down spacing: 10
        draw_bg.color: #202430

        header := View{width: Fill height: Fit flow: Right spacing: 8 align: Align{y: 0.5}
            title := Label{text: "App" draw_text.color: #fff draw_text.text_style.font_size: 16}
            status := Label{text: "Ready" draw_text.color: #9fb1d8 draw_text.text_style.font_size: 11}
            filler := Filler{}
            close_button := Button{text: "Close"}
        }

        app_canvas := RoundedView{
            width: Fill height: Fit padding: 12 new_batch: true
            draw_bg.color: #131722
            splash_view := mod.widgets.AgentSplash{width: Fill height: Fit}
        }

        source_label := Label{text: "Splash Source" draw_text.color: #d6dcee draw_text.text_style.font_size: 11}
        source := TextInput{
            width: Fill height: 160
            is_multiline: true
            is_read_only: true
            empty_text: "No Splash source yet"
        }

        prompt_row := View{width: Fill height: Fit flow: Right spacing: 8 align: Align{y: 1.0}
            prompt_input := TextInput{
                width: Fill height: 64
                is_multiline: true
                empty_text: "Ask for an inference round-trip for this app..."
            }
            send_button := Button{text: "Send" width: 90}
        }

        last_request_label := Label{text: "Last Request" draw_text.color: #d6dcee draw_text.text_style.font_size: 11}
        last_request := TextInput{
            width: Fill height: 72
            is_multiline: true
            is_read_only: true
            empty_text: "No request yet"
        }

        last_response_label := Label{text: "Last Response" draw_text.color: #d6dcee draw_text.text_style.font_size: 11}
        last_response := TextInput{
            width: Fill height: 120
            is_multiline: true
            is_read_only: true
            empty_text: "No response yet"
        }
    }

    startup() do #(MakepadRootApp::script_component(vm)){
        ui: Root{
            main_window := Window{
                window.inner_size: vec2(1120, 860)
                window.title: "LSP Agent Makepad Host"
                body +: {
                    width: Fill
                    height: Fill
                    flow: Down
                    padding: 16
                    spacing: 12

                    HostHeader{}

                    status_line := Label{
                        text: "Waiting for launched apps..."
                        draw_text.color: #9ca7c2
                        draw_text.text_style.font_size: 11
                    }

                    apps_scroll := ScrollYView{
                        width: Fill
                        height: Fill
                        flow: Down
                        spacing: 12

                        empty_state := EmptyState{}
                        slot_0 := AppSlot{}
                        slot_1 := AppSlot{}
                        slot_2 := AppSlot{}
                        slot_3 := AppSlot{}
                        slot_4 := AppSlot{}
                        slot_5 := AppSlot{}
                        slot_6 := AppSlot{}
                        slot_7 := AppSlot{}
                    }
                }
            }
        }
    }
}

#[derive(Clone)]
struct AppWindowSnapshot {
    id: String,
    splash_source: String,
    last_request: Option<String>,
    last_response: Option<String>,
    request_in_flight: bool,
}

impl AppWindowSnapshot {
    fn from_state(id: &str, state: &AppState) -> Self {
        Self {
            id: id.to_string(),
            splash_source: state.content.clone(),
            last_request: state.last_request.clone(),
            last_response: state.last_response.clone(),
            request_in_flight: state.request_in_flight,
        }
    }
}

fn log_preview(content: &str) -> String {
    let mut preview: String = content.chars().take(120).collect();
    preview = preview.replace('\n', "\\n");
    if content.chars().count() > 120 {
        preview.push_str("...");
    }
    preview
}

#[derive(Script, ScriptHook)]
pub struct MakepadRootApp {
    #[live]
    ui: WidgetRef,
    #[rust]
    slot_app_ids: Vec<Option<String>>,
    #[rust]
    pending_responses: HashMap<String, oneshot::Receiver<String>>,
    #[rust]
    last_synced_revision: u64,
}

impl MakepadRootApp {
    fn host_revision() -> u64 {
        HOST_STATE.read().unwrap().revision
    }

    fn drain_snapshots() -> (u64, Vec<AppWindowSnapshot>) {
        let mut state = HOST_STATE.write().unwrap();
        let launches: Vec<_> = state.pending_launches.drain(..).collect();

        if !launches.is_empty() {
            let launch_ids: Vec<_> = launches.iter().map(|launch| launch.id.as_str()).collect();
            eprintln!(
                "[LSP Agent Host UI] Draining {} pending launch(es): {:?}",
                launches.len(),
                launch_ids
            );
        }

        for launch in launches {
            if !state.app_order.iter().any(|id| id == &launch.id) {
                state.app_order.push(launch.id.clone());
            }

            state
                .apps
                .entry(launch.id)
                .and_modify(|app| app.content = launch.content.clone())
                .or_insert_with(|| AppState::new(launch.content));
        }

        let revision = state.revision;
        let snapshots = state
            .app_order
            .iter()
            .filter_map(|id| {
                state
                    .apps
                    .get(id)
                    .map(|app| AppWindowSnapshot::from_state(id, app))
            })
            .collect();

        (revision, snapshots)
    }

    fn slot_widget(&self, cx: &mut Cx, index: usize) -> WidgetRef {
        match index {
            0 => self.ui.widget(cx, ids!(slot_0)),
            1 => self.ui.widget(cx, ids!(slot_1)),
            2 => self.ui.widget(cx, ids!(slot_2)),
            3 => self.ui.widget(cx, ids!(slot_3)),
            4 => self.ui.widget(cx, ids!(slot_4)),
            5 => self.ui.widget(cx, ids!(slot_5)),
            6 => self.ui.widget(cx, ids!(slot_6)),
            7 => self.ui.widget(cx, ids!(slot_7)),
            _ => panic!("invalid app slot index: {index}"),
        }
    }

    fn clear_slot(&mut self, cx: &mut Cx, index: usize) {
        let slot = self.slot_widget(cx, index);
        slot.set_visible(cx, false);
        slot.label(cx, ids!(title)).set_text(cx, "");
        slot.label(cx, ids!(status)).set_text(cx, "");
        slot.widget(cx, ids!(source)).set_text(cx, "");
        slot.widget(cx, ids!(last_request)).set_text(cx, "");
        slot.widget(cx, ids!(last_response)).set_text(cx, "");
        slot.text_input(cx, ids!(prompt_input)).set_text(cx, "");

        let canvas = slot.widget(cx, ids!(app_canvas));
        canvas.widget(cx, ids!(splash_view)).set_text(cx, "");

        if index < self.slot_app_ids.len() {
            self.slot_app_ids[index] = None;
        }
    }

    fn render_slot(&mut self, cx: &mut Cx, index: usize, snapshot: &AppWindowSnapshot) {
        eprintln!(
            "[LSP Agent Host UI] Rendering slot {} for {} ({} chars): {}",
            index,
            snapshot.id,
            snapshot.splash_source.chars().count(),
            log_preview(&snapshot.splash_source)
        );
        let slot = self.slot_widget(cx, index);
        slot.set_visible(cx, true);
        slot.label(cx, ids!(title))
            .set_text(cx, &format!("App {}", snapshot.id));
        slot.label(cx, ids!(status)).set_text(
            cx,
            if snapshot.request_in_flight {
                "Waiting for inference response"
            } else {
                "Ready"
            },
        );
        slot.widget(cx, ids!(source))
            .set_text(cx, &snapshot.splash_source);
        slot.widget(cx, ids!(last_request))
            .set_text(cx, snapshot.last_request.as_deref().unwrap_or_default());
        slot.widget(cx, ids!(last_response))
            .set_text(cx, snapshot.last_response.as_deref().unwrap_or_default());

        let canvas = slot.widget(cx, ids!(app_canvas));
        canvas
            .widget(cx, ids!(splash_view))
            .set_text(cx, &snapshot.splash_source);

        if index < self.slot_app_ids.len() {
            self.slot_app_ids[index] = Some(snapshot.id.clone());
        }
    }

    fn slot_index_for_app(&self, app_id: &str) -> Option<usize> {
        self.slot_app_ids
            .iter()
            .position(|slot| slot.as_deref() == Some(app_id))
    }

    fn sync_from_host_state(&mut self, cx: &mut Cx) {
        let (revision, snapshots) = Self::drain_snapshots();
        let visible_count = snapshots.len().min(MAX_APP_SLOTS);
        self.last_synced_revision = revision;

        if !snapshots.is_empty() {
            let app_ids: Vec<_> = snapshots
                .iter()
                .map(|snapshot| snapshot.id.as_str())
                .collect();
            eprintln!(
                "[LSP Agent Host UI] sync_from_host_state -> revision={}, {} snapshot(s), visible_count={}, app_ids={:?}",
                revision,
                snapshots.len(),
                visible_count,
                app_ids
            );
        }

        for (index, snapshot) in snapshots.iter().take(MAX_APP_SLOTS).enumerate() {
            self.render_slot(cx, index, snapshot);
        }

        for index in visible_count..MAX_APP_SLOTS {
            self.clear_slot(cx, index);
        }

        self.ui
            .widget(cx, ids!(empty_state))
            .set_visible(cx, snapshots.is_empty());

        let status = if snapshots.is_empty() {
            "Waiting for launched apps...".to_string()
        } else if snapshots.len() > MAX_APP_SLOTS {
            format!(
                "Showing the first {} of {} launched apps.",
                MAX_APP_SLOTS,
                snapshots.len()
            )
        } else {
            format!(
                "Showing {} live Splash app{}.",
                snapshots.len(),
                if snapshots.len() == 1 { "" } else { "s" }
            )
        };

        self.ui.widget(cx, ids!(status_line)).set_text(cx, &status);
    }

    fn start_inference(&mut self, cx: &mut Cx, app_id: String, prompt: String) {
        let prompt = prompt.trim().to_string();
        if prompt.is_empty() {
            return;
        }

        let (tx, rx) = oneshot::channel();
        {
            let mut state = HOST_STATE.write().unwrap();
            let Some(app) = state.apps.get_mut(&app_id) else {
                return;
            };

            app.last_request = Some(prompt.clone());
            app.request_in_flight = true;
            app.pending_inference.push_back(tx);
        }

        if let Err(error) = send_command(HostCommand::Inference {
            app_id: app_id.clone(),
            content: prompt,
        }) {
            let mut state = HOST_STATE.write().unwrap();
            if let Some(app) = state.apps.get_mut(&app_id) {
                app.request_in_flight = false;
                let _ = app.pending_inference.pop_back();
                app.last_response = Some(error);
            }
            drop(state);
            self.sync_from_host_state(cx);
            return;
        }

        self.pending_responses.insert(app_id.clone(), rx);

        if let Some(index) = self.slot_index_for_app(&app_id) {
            self.slot_widget(cx, index)
                .text_input(cx, ids!(prompt_input))
                .set_text(cx, "");
        }

        self.sync_from_host_state(cx);
    }

    fn close_app(&mut self, cx: &mut Cx, app_id: String) {
        let _ = send_command(HostCommand::CloseApp(app_id.clone()));
        self.pending_responses.remove(&app_id);

        let mut state = HOST_STATE.write().unwrap();
        state.apps.remove(&app_id);
        state.app_order.retain(|id| id != &app_id);
        drop(state);

        self.sync_from_host_state(cx);
    }

    fn poll_inference_responses(&mut self, cx: &mut Cx) {
        let mut completed = Vec::new();
        let mut closed = Vec::new();

        for (app_id, rx) in self.pending_responses.iter_mut() {
            match rx.try_recv() {
                Ok(_) => completed.push(app_id.clone()),
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {
                    closed.push(app_id.clone())
                }
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => {}
            }
        }

        if !closed.is_empty() {
            let mut state = HOST_STATE.write().unwrap();
            for app_id in &closed {
                if let Some(app) = state.apps.get_mut(app_id) {
                    app.request_in_flight = false;
                    app.last_response = Some("Inference response channel closed.".to_string());
                }
            }
        }

        let had_updates = !completed.is_empty() || !closed.is_empty();

        for app_id in completed.into_iter().chain(closed) {
            self.pending_responses.remove(&app_id);
        }

        if had_updates {
            // Keep the visible host state fresh only when inference state changed.
            self.sync_from_host_state(cx);
        }
    }
}

impl MatchEvent for MakepadRootApp {
    fn handle_actions(&mut self, cx: &mut Cx, actions: &Actions) {
        for index in 0..MAX_APP_SLOTS {
            let Some(app_id) = self.slot_app_ids.get(index).and_then(|slot| slot.clone()) else {
                continue;
            };

            let slot = self.slot_widget(cx, index);

            if let Some((prompt, _)) = slot.text_input(cx, ids!(prompt_input)).returned(actions) {
                self.start_inference(cx, app_id.clone(), prompt);
            }

            if slot.button(cx, ids!(send_button)).clicked(actions) {
                let prompt = slot.text_input(cx, ids!(prompt_input)).text();
                self.start_inference(cx, app_id.clone(), prompt);
            }

            if slot.button(cx, ids!(close_button)).clicked(actions) {
                self.close_app(cx, app_id);
            }
        }
    }
}

impl AppMain for MakepadRootApp {
    fn script_mod(vm: &mut ScriptVm) -> ScriptValue {
        makepad_widgets::script_mod(vm);
        self::script_mod(vm)
    }

    fn after_new_from_script(_vm: &mut ScriptVm, app: &mut Self) {
        app.slot_app_ids = vec![None; MAX_APP_SLOTS];
        app.pending_responses = HashMap::new();
        app.last_synced_revision = 0;
    }

    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.poll_inference_responses(cx);
        self.match_event(cx, event);
        self.ui.handle_event(cx, event, &mut Scope::empty());

        if matches!(event, Event::Startup) {
            self.sync_from_host_state(cx);
            return;
        }

        if matches!(event, Event::Signal) {
            let revision = Self::host_revision();
            if revision != self.last_synced_revision {
                self.sync_from_host_state(cx);
            }
        }
    }
}
