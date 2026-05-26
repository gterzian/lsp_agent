use makepad_widgets::*;

use crate::app_window::{AppWindowCardRef, AppWindowCardWidgetRefExt, AppWindowSnapshot};
use crate::state::{AppState, HOST_STATE};

live_design! {
    use link::theme::*;
    use link::widgets::*;
    use crate::app_window::AppWindowCard;

    MakepadRootApp = {{MakepadRootApp}} {
        ui: <Root> {
            main_window = <Window> {
                body = <View> {
                    width: Fill,
                    height: Fill,
                    flow: Down,
                    spacing: 12,
                    padding: {left: 20, top: 20, right: 20, bottom: 20}
                    show_bg: true,
                    draw_bg: {
                        color: (THEME_COLOR_BG_APP)
                    }

                    title = <Label> {
                        text: "Makepad App Host"
                        draw_text: {
                            text_style: <THEME_FONT_BOLD> {
                                font_size: 26
                            }
                        }
                    }

                    subtitle = <Label> {
                        text: "Launched apps appear below. This host shows source and supports manual inference round-trips."
                    }

                    status_line = <Label> {
                        text: "Waiting for launched apps..."
                    }

                    slot_0 = <AppWindowCard> {}
                    slot_1 = <AppWindowCard> {}
                    slot_2 = <AppWindowCard> {}
                    slot_3 = <AppWindowCard> {}
                    slot_4 = <AppWindowCard> {}
                    slot_5 = <AppWindowCard> {}
                    slot_6 = <AppWindowCard> {}
                    slot_7 = <AppWindowCard> {}
                }
            }
        }
    }
}

#[derive(Live, LiveHook)]
pub struct MakepadRootApp {
    #[live]
    ui: WidgetRef,
}

impl LiveRegister for MakepadRootApp {
    fn live_register(cx: &mut Cx) {
        crate::live_design(cx);
    }
}

impl MakepadRootApp {
    fn drain_snapshots() -> Vec<AppWindowSnapshot> {
        let mut state = HOST_STATE.write().unwrap();
        let launches: Vec<_> = state.pending_launches.drain(..).collect();

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

        state
            .app_order
            .iter()
            .filter_map(|id| {
                state
                    .apps
                    .get(id)
                    .map(|app| AppWindowSnapshot::from_state(id, app))
            })
            .collect()
    }

    fn slots(&self) -> [AppWindowCardRef; 8] {
        [
            self.ui.widget(id!(slot_0)).as_app_window_card(),
            self.ui.widget(id!(slot_1)).as_app_window_card(),
            self.ui.widget(id!(slot_2)).as_app_window_card(),
            self.ui.widget(id!(slot_3)).as_app_window_card(),
            self.ui.widget(id!(slot_4)).as_app_window_card(),
            self.ui.widget(id!(slot_5)).as_app_window_card(),
            self.ui.widget(id!(slot_6)).as_app_window_card(),
            self.ui.widget(id!(slot_7)).as_app_window_card(),
        ]
    }

    fn sync_from_host_state(&mut self, cx: &mut Cx) {
        let snapshots = Self::drain_snapshots();
        let slots = self.slots();

        for (slot, snapshot) in slots.iter().zip(snapshots.iter()) {
            slot.set_snapshot(cx, Some(snapshot));
        }

        for slot in slots.iter().skip(snapshots.len()) {
            slot.set_snapshot(cx, None);
        }

        let status = if snapshots.is_empty() {
            "Waiting for launched apps...".to_string()
        } else if snapshots.len() > slots.len() {
            format!(
                "Showing the first {} of {} apps.",
                slots.len(),
                snapshots.len()
            )
        } else {
            format!(
                "Showing {} app{}.",
                snapshots.len(),
                if snapshots.len() == 1 { "" } else { "s" }
            )
        };

        self.ui.widget(id!(status_line)).set_text(cx, &status);
    }
}

impl AppMain for MakepadRootApp {
    fn handle_event(&mut self, cx: &mut Cx, event: &Event) {
        self.sync_from_host_state(cx);
        self.ui.handle_event(cx, event, &mut Scope::empty());
        self.sync_from_host_state(cx);
    }
}
