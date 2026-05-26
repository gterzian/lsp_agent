pub mod app;
pub mod app_window;
pub mod host;
pub mod state;

use std::sync::Arc;

use makepad_widgets::{AppMain, Cx, CxOsApi, Event, LiveNew, SignalToUI};
use tokio::sync::mpsc;

use crate::app::MakepadRootApp;
use crate::host::MakepadAppHost;
use crate::state::{HostCommand, COMMAND_TX};

pub fn live_design(cx: &mut Cx) {
    makepad_widgets::live_design(cx);
    crate::app_window::live_design(cx);
    crate::app::live_design(cx);
}

makepad_widgets::app_main!(MakepadRootApp);

pub fn run() {
    start_backend_thread();
    app_main();
}

fn start_backend_thread() {
    let host = Arc::new(MakepadAppHost {
        signal: SignalToUI::new(),
    });

    std::thread::spawn(move || {
        let runtime = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
        runtime.block_on(async move {
            let (agent, mut exit_rx) = agent::start_app_backend(host).await;
            let (command_tx, mut command_rx) = mpsc::unbounded_channel::<HostCommand>();
            let _ = COMMAND_TX.set(command_tx);

            loop {
                tokio::select! {
                    Some(command) = command_rx.recv() => {
                        match command {
                            HostCommand::Inference { app_id, content } => {
                                agent.app_inference_request(content, app_id).await;
                            }
                            HostCommand::CloseApp(app_id) => {
                                agent.close_app(app_id).await;
                            }
                        }
                    }
                    _ = exit_rx.recv() => {
                        break;
                    }
                }
            }
        });
    });
}
