mod document;
pub mod prompts;

pub use document::{
    AgentRequest, AgentResponse, ConversationFragment, DocumentContent, DocumentManager, Id,
    LspAgent, NoStorage, StoredValue, Uri,
};

use automerge_repo::{ConnDirection, DocHandle, DocumentId, Repo, RepoHandle};
use autosurgeon::{hydrate, reconcile};
use serde::Deserialize;
use std::io::ErrorKind;
use std::net::TcpListener as StdTcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::process::{Child, Command};
use tokio::runtime::Handle;
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Duration, sleep};
use traits::{App, InferenceClient, WebAgent, WorkspaceAgent};
use uuid::Uuid;

fn find_repo_root(exe_path: &std::path::Path) -> Option<std::path::PathBuf> {
    for ancestor in exe_path.ancestors() {
        let candidate = ancestor.join("Cargo.toml");
        if let Ok(contents) = std::fs::read_to_string(&candidate)
            && contents.contains("[workspace]")
        {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

const PEER1_PORT: u16 = 2341;
const DOC_ID_PORT: u16 = 2348;
const DEFAULT_TOOL_MAX_ITERATIONS: usize = 3;
const STARTUP_BIND_MAX_ATTEMPTS: usize = 40;
const STARTUP_BIND_RETRY_DELAY_MS: u64 = 250;
const CANONICAL_MAKEPAD_TODO_APP: &str = r#"let todos = [
    {text: "Buy groceries" tag: "errands" done: false}
    {text: "Write unit tests" tag: "dev" done: false}
]
let max_todos = 5

fn remaining_count(){
    let count = 0
    for todo in todos {
        if !todo.done count += 1
    }
    count
}

fn sync_status(){
    ui.todo_status.set_text(remaining_count() + " remaining / " + todos.len() + " total (5 slots)")
}

fn sync_row_0(){
    if 0 < todos.len() {
        let todo = todos[0]
        let marker = "[ ]"
        if todo.done { marker = "[x]" }
        ui.todo_row_0.marker.set_text(marker)
        ui.todo_row_0.label.set_text(todo.text)
        ui.todo_row_0.tag.set_text(todo.tag)
    } else {
        ui.todo_row_0.marker.set_text(".")
        ui.todo_row_0.label.set_text("Empty slot")
        ui.todo_row_0.tag.set_text("")
    }
}

fn sync_row_1(){
    if 1 < todos.len() {
        let todo = todos[1]
        let marker = "[ ]"
        if todo.done { marker = "[x]" }
        ui.todo_row_1.marker.set_text(marker)
        ui.todo_row_1.label.set_text(todo.text)
        ui.todo_row_1.tag.set_text(todo.tag)
    } else {
        ui.todo_row_1.marker.set_text(".")
        ui.todo_row_1.label.set_text("Empty slot")
        ui.todo_row_1.tag.set_text("")
    }
}

fn sync_row_2(){
    if 2 < todos.len() {
        let todo = todos[2]
        let marker = "[ ]"
        if todo.done { marker = "[x]" }
        ui.todo_row_2.marker.set_text(marker)
        ui.todo_row_2.label.set_text(todo.text)
        ui.todo_row_2.tag.set_text(todo.tag)
    } else {
        ui.todo_row_2.marker.set_text(".")
        ui.todo_row_2.label.set_text("Empty slot")
        ui.todo_row_2.tag.set_text("")
    }
}

fn sync_row_3(){
    if 3 < todos.len() {
        let todo = todos[3]
        let marker = "[ ]"
        if todo.done { marker = "[x]" }
        ui.todo_row_3.marker.set_text(marker)
        ui.todo_row_3.label.set_text(todo.text)
        ui.todo_row_3.tag.set_text(todo.tag)
    } else {
        ui.todo_row_3.marker.set_text(".")
        ui.todo_row_3.label.set_text("Empty slot")
        ui.todo_row_3.tag.set_text("")
    }
}

fn sync_row_4(){
    if 4 < todos.len() {
        let todo = todos[4]
        let marker = "[ ]"
        if todo.done { marker = "[x]" }
        ui.todo_row_4.marker.set_text(marker)
        ui.todo_row_4.label.set_text(todo.text)
        ui.todo_row_4.tag.set_text(todo.tag)
    } else {
        ui.todo_row_4.marker.set_text(".")
        ui.todo_row_4.label.set_text("Empty slot")
        ui.todo_row_4.tag.set_text("")
    }
}

fn sync_rows(){
    sync_row_0()
    sync_row_1()
    sync_row_2()
    sync_row_3()
    sync_row_4()
    sync_status()
}

fn add_todo(text){
    let clean = ("" + text).trim()
    if clean == "" { return }
    if todos.len() >= max_todos {
        ui.todo_status.set_text("List is full (5 slots max)")
        return
    }
    todos.push({text: clean tag: "" done: false})
    ui.todo_input.set_text("")
    sync_rows()
}

fn toggle_todo(index){
    if index >= todos.len() { return }
    let next_done = !todos[index].done
    todos[index] += {done: next_done}
    sync_rows()
}

fn delete_todo(index){
    if index >= todos.len() { return }
    todos.remove(index)
    sync_rows()
}

fn clear_done(){
    todos.retain(|todo| !todo.done)
    sync_rows()
}

let TodoRow = RoundedView{
    width: Fill height: Fit
    padding: Inset{top: 8 bottom: 8 left: 12 right: 12}
    flow: Right spacing: 10
    align: Align{y: 0.5}
    new_batch: true
    draw_bg.color: #x2a2a3a
    draw_bg.border_radius: 8.0
    marker := Label{text: "[ ]" width: 24 draw_text.color: #x8fb7ff draw_text.text_style.font_size: 11}
    label := Label{text: "task" width: Fill draw_text.color: #ddd draw_text.text_style.font_size: 11}
    tag := Label{text: "" draw_text.color: #888 draw_text.text_style.font_size: 9}
    toggle := ButtonFlatter{text: "Toggle" width: 56 height: 28 draw_text.color: #9fb1d8}
    delete := ButtonFlatter{text: "Delete" width: 56 height: 28 draw_text.color: #888}
}

RoundedView{
    width: Fill height: Fit
    flow: Down spacing: 10
    padding: 16
    new_batch: true
    draw_bg.color: #x1e1e2e
    draw_bg.border_radius: 10.0
    Label{text: "My Tasks" draw_text.color: #fff draw_text.text_style.font_size: 14}
    View{
        width: Fill height: Fit
        flow: Right spacing: 8
        align: Align{y: 0.5}
        todo_input := TextInput{
            width: Fill height: 34
            empty_text: "Add a new task"
            on_return: |text| add_todo(text)
        }
        Button{text: "Add" width: 64 height: 34 on_click: || add_todo(ui.todo_input.text())}
    }
    View{
        width: Fill height: Fit
        flow: Down spacing: 4
        todo_row_0 := TodoRow{
            label.text: "Buy groceries"
            tag.text: "errands"
            toggle.on_click: || toggle_todo(0)
            delete.on_click: || delete_todo(0)
        }
        todo_row_1 := TodoRow{
            label.text: "Write unit tests"
            tag.text: "dev"
            toggle.on_click: || toggle_todo(1)
            delete.on_click: || delete_todo(1)
        }
        todo_row_2 := TodoRow{
            marker.text: "."
            label.text: "Empty slot"
            tag.text: ""
            toggle.on_click: || toggle_todo(2)
            delete.on_click: || delete_todo(2)
        }
        todo_row_3 := TodoRow{
            marker.text: "."
            label.text: "Empty slot"
            tag.text: ""
            toggle.on_click: || toggle_todo(3)
            delete.on_click: || delete_todo(3)
        }
        todo_row_4 := TodoRow{
            marker.text: "."
            label.text: "Empty slot"
            tag.text: ""
            toggle.on_click: || toggle_todo(4)
            delete.on_click: || delete_todo(4)
        }
    }
    View{
        width: Fill height: Fit
        flow: Right
        align: Align{y: 0.5}
        todo_status := Label{text: "2 remaining / 2 total (5 slots)" width: Fill draw_text.color: #aaa}
        ButtonFlatter{text: "Clear completed" on_click: || clear_done()}
    }
}"#;
const CANONICAL_MAKEPAD_NOTES_APP: &str = r#"let notes = [
    {text: "Pick up dry cleaning"}
    {text: "Outline release checklist"}
]
let max_notes = 5

fn sync_status(){
    ui.note_status.set_text(notes.len() + " notes (5 slots)")
}

fn sync_row_0(){
    if 0 < notes.len() {
        ui.note_row_0.label.set_text(notes[0].text)
    } else {
        ui.note_row_0.label.set_text("Empty slot")
    }
}

fn sync_row_1(){
    if 1 < notes.len() {
        ui.note_row_1.label.set_text(notes[1].text)
    } else {
        ui.note_row_1.label.set_text("Empty slot")
    }
}

fn sync_row_2(){
    if 2 < notes.len() {
        ui.note_row_2.label.set_text(notes[2].text)
    } else {
        ui.note_row_2.label.set_text("Empty slot")
    }
}

fn sync_row_3(){
    if 3 < notes.len() {
        ui.note_row_3.label.set_text(notes[3].text)
    } else {
        ui.note_row_3.label.set_text("Empty slot")
    }
}

fn sync_row_4(){
    if 4 < notes.len() {
        ui.note_row_4.label.set_text(notes[4].text)
    } else {
        ui.note_row_4.label.set_text("Empty slot")
    }
}

fn sync_rows(){
    sync_row_0()
    sync_row_1()
    sync_row_2()
    sync_row_3()
    sync_row_4()
    sync_status()
}

fn add_note(text){
    let clean = ("" + text).trim()
    if clean == "" { return }
    if notes.len() >= max_notes {
        ui.note_status.set_text("List is full (5 slots max)")
        return
    }
    notes.push({text: clean})
    ui.note_input.set_text("")
    sync_rows()
}

fn delete_note(index){
    if index >= notes.len() { return }
    notes.remove(index)
    sync_rows()
}

fn clear_all(){
    notes.retain(|note| false)
    sync_rows()
}

let NoteRow = RoundedView{
    width: Fill height: Fit
    padding: Inset{top: 8 bottom: 8 left: 12 right: 12}
    flow: Right spacing: 10
    align: Align{y: 0.5}
    new_batch: true
    draw_bg.color: #x2a2a3a
    draw_bg.border_radius: 8.0
    label := Label{text: "note" width: Fill draw_text.color: #ddd draw_text.text_style.font_size: 11}
    delete := ButtonFlatter{text: "Delete" width: 56 height: 28 draw_text.color: #888}
}

RoundedView{
    width: Fill height: Fit
    flow: Down spacing: 10
    padding: 16
    new_batch: true
    draw_bg.color: #x1e1e2e
    draw_bg.border_radius: 10.0
    Label{text: "Quick Notes" draw_text.color: #fff draw_text.text_style.font_size: 14}
    View{
        width: Fill height: Fit
        flow: Right spacing: 8
        align: Align{y: 0.5}
        note_input := TextInput{
            width: Fill height: 34
            empty_text: "Write something down"
            on_return: |text| add_note(text)
        }
        Button{text: "Add" width: 64 height: 34 on_click: || add_note(ui.note_input.text())}
    }
    View{
        width: Fill height: Fit
        flow: Down spacing: 4
        note_row_0 := NoteRow{
            label.text: "Pick up dry cleaning"
            delete.on_click: || delete_note(0)
        }
        note_row_1 := NoteRow{
            label.text: "Outline release checklist"
            delete.on_click: || delete_note(1)
        }
        note_row_2 := NoteRow{
            label.text: "Empty slot"
            delete.on_click: || delete_note(2)
        }
        note_row_3 := NoteRow{
            label.text: "Empty slot"
            delete.on_click: || delete_note(3)
        }
        note_row_4 := NoteRow{
            label.text: "Empty slot"
            delete.on_click: || delete_note(4)
        }
    }
    View{
        width: Fill height: Fit
        flow: Right
        align: Align{y: 0.5}
        note_status := Label{text: "2 notes (5 slots)" width: Fill draw_text.color: #aaa}
        ButtonFlatter{text: "Clear all" on_click: || clear_all()}
    }
}"#;

#[derive(Clone, Copy)]
struct StandardMakepadApp {
    id: &'static str,
    description: &'static str,
    content: &'static str,
}

const STANDARD_MAKEPAD_APPS: &[StandardMakepadApp] = &[
    StandardMakepadApp {
        id: "todo",
        description: "Well-written task list app with add, toggle, delete, and clear-completed flows.",
        content: CANONICAL_MAKEPAD_TODO_APP,
    },
    StandardMakepadApp {
        id: "notes",
        description: "Well-written quick notes app with add, delete, and clear-all flows.",
        content: CANONICAL_MAKEPAD_NOTES_APP,
    },
];

fn standard_makepad_apps(runtime: AppRuntime) -> &'static [StandardMakepadApp] {
    if runtime == AppRuntime::Makepad {
        STANDARD_MAKEPAD_APPS
    } else {
        &[]
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

fn bind_local_listener(port: u16, label: &str) -> StdTcpListener {
    let addr = format!("127.0.0.1:{port}");

    for attempt in 1..=STARTUP_BIND_MAX_ATTEMPTS {
        match StdTcpListener::bind(&addr) {
            Ok(listener) => {
                listener
                    .set_nonblocking(true)
                    .expect("failed to configure startup listener");
                eprintln!(
                    "[LSP Agent] Bound {} on {} (attempt {}/{}).",
                    label, addr, attempt, STARTUP_BIND_MAX_ATTEMPTS
                );
                return listener;
            }
            Err(error)
                if error.kind() == ErrorKind::AddrInUse && attempt < STARTUP_BIND_MAX_ATTEMPTS =>
            {
                eprintln!(
                    "[LSP Agent] {} on {} is still in use. Waiting for the previous server to release it ({}/{}).",
                    label, addr, attempt, STARTUP_BIND_MAX_ATTEMPTS
                );
                std::thread::sleep(Duration::from_millis(STARTUP_BIND_RETRY_DELAY_MS));
            }
            Err(error) => {
                panic!(
                    "[LSP Agent] Failed to bind {} on {} after {} attempt(s): {}",
                    label, addr, attempt, error
                );
            }
        }
    }

    panic!(
        "[LSP Agent] Failed to bind {} on {} after {} attempt(s).",
        label, addr, STARTUP_BIND_MAX_ATTEMPTS
    );
}

fn standard_makepad_app_infos(runtime: AppRuntime) -> Option<Vec<prompts::StandardAppInfo>> {
    let apps = standard_makepad_apps(runtime);
    if apps.is_empty() {
        return None;
    }

    Some(
        apps.iter()
            .map(|app| prompts::StandardAppInfo {
                id: app.id.to_string(),
                description: app.description.to_string(),
            })
            .collect(),
    )
}

fn standard_makepad_app_by_id(runtime: AppRuntime, id: &str) -> Option<StandardMakepadApp> {
    standard_makepad_apps(runtime)
        .iter()
        .copied()
        .find(|app| app.id == id)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AppRuntime {
    Web,
    Makepad,
}

impl AppRuntime {
    pub fn from_env() -> Self {
        match std::env::var("LSP_AGENT_APP_RUNTIME") {
            Ok(value) if value.eq_ignore_ascii_case("makepad") => Self::Makepad,
            Ok(value) if value.eq_ignore_ascii_case("web") => Self::Web,
            Ok(value) => {
                eprintln!(
                    "[LSP Agent] Unknown app runtime '{}', falling back to 'web'.",
                    value
                );
                Self::Web
            }
            Err(_) => Self::Makepad,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Web => "web",
            Self::Makepad => "makepad",
        }
    }

    fn host_label(self) -> &'static str {
        match self {
            Self::Web => "web host",
            Self::Makepad => "makepad host",
        }
    }

    fn candidate_binaries(self, project_root: &Path) -> Vec<PathBuf> {
        let mut candidates = Vec::new();

        if let Ok(override_path) = std::env::var("LSP_AGENT_APP_BINARY") {
            candidates.push(PathBuf::from(override_path));
        }

        match self {
            Self::Web => {
                if let Ok(override_path) = std::env::var("LSP_AGENT_WEB_BINARY") {
                    candidates.push(PathBuf::from(override_path));
                }
                candidates.push(project_root.join("target/debug/web"));
                candidates.push(project_root.join("web/target/debug/web"));
            }
            Self::Makepad => {
                if let Ok(override_path) = std::env::var("LSP_AGENT_MAKEPAD_BINARY") {
                    candidates.push(PathBuf::from(override_path));
                }
                candidates.push(project_root.join("target/debug/makepad-host"));
                candidates.push(project_root.join("bins/makepad-host/target/debug/makepad-host"));
            }
        }

        candidates
    }
}

#[derive(Deserialize, Debug)]
struct ToolResponse {
    action: String,
    message: Option<String>,
    app: Option<String>,
    standard_app_id: Option<String>,
}

fn validate_makepad_splash_body(body: &str) -> Option<String> {
    if body.contains("if (") {
        return Some(
            "it used parenthesized `if` conditions; use `if cond { ... }` syntax instead"
                .to_string(),
        );
    }

    let mut brace_depth = 0usize;
    for line in body.lines() {
        let trimmed = line.trim();
        let normalized = trimmed.trim_end_matches(';').trim();

        if brace_depth == 0
            && !normalized.is_empty()
            && !normalized.starts_with("//")
            && !normalized.starts_with("let ")
            && !normalized.starts_with("fn ")
            && !normalized.starts_with("if ")
            && !normalized.starts_with("else")
        {
            let is_top_level_ui_call = normalized.starts_with("ui.")
                && normalized.contains('(')
                && normalized.ends_with(')');
            let is_top_level_helper_call = normalized
                .find('(')
                .map(|index| {
                    let before_paren = normalized[..index].trim();
                    !before_paren.is_empty()
                        && before_paren
                            .chars()
                            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
                        && normalized.ends_with(')')
                })
                .unwrap_or(false);

            if is_top_level_ui_call || is_top_level_helper_call {
                return Some(
                    "it tried to run top-level initialization code like `sync_rows()`; the root container must be the final top-level expression, and initial widget values must be seeded directly in the declared UI"
                        .to_string(),
                );
            }
        }

        for ch in line.chars() {
            match ch {
                '{' => brace_depth += 1,
                '}' => brace_depth = brace_depth.saturating_sub(1),
                _ => {}
            }
        }
    }

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }

        if trimmed.starts_with("on_render:") {
            return Some(
                "it used `on_render`, which currently destabilizes embedded Makepad mini apps; declare fixed named widgets and update them directly instead"
                    .to_string(),
            );
        }
    }

    let mut inside_text_input = false;
    let mut text_input_brace_depth = 0usize;
    let mut text_input_has_height = false;

    for line in body.lines() {
        let trimmed = line.trim();

        if !inside_text_input {
            let Some(start) = trimmed
                .find("TextInput{")
                .or_else(|| trimmed.find("TextInput {"))
            else {
                continue;
            };

            inside_text_input = true;
            text_input_brace_depth = 0;
            text_input_has_height = false;

            let snippet = &trimmed[start..];
            if !trimmed.starts_with("//") && snippet.contains("height:") {
                text_input_has_height = true;
                if snippet.contains("height: Fit") {
                    return Some(
                        "it used `TextInput` with `height: Fit`; embedded Makepad text inputs must use a fixed numeric height such as `34`"
                            .to_string(),
                    );
                }
            }

            for ch in snippet.chars() {
                match ch {
                    '{' => text_input_brace_depth += 1,
                    '}' => text_input_brace_depth = text_input_brace_depth.saturating_sub(1),
                    _ => {}
                }
            }

            if text_input_brace_depth == 0 {
                if !text_input_has_height {
                    return Some(
                        "it declared `TextInput` without an explicit fixed height; use a numeric height such as `34` in embedded Makepad apps"
                            .to_string(),
                    );
                }
                inside_text_input = false;
            }

            continue;
        }

        if !trimmed.starts_with("//") && trimmed.contains("height:") {
            text_input_has_height = true;
            if trimmed.contains("height: Fit") {
                return Some(
                    "it used `TextInput` with `height: Fit`; embedded Makepad text inputs must use a fixed numeric height such as `34`"
                        .to_string(),
                );
            }
        }

        for ch in trimmed.chars() {
            match ch {
                '{' => text_input_brace_depth += 1,
                '}' => text_input_brace_depth = text_input_brace_depth.saturating_sub(1),
                _ => {}
            }
        }

        if text_input_brace_depth == 0 {
            if !text_input_has_height {
                return Some(
                    "it declared `TextInput` without an explicit fixed height; use a numeric height such as `34` in embedded Makepad apps"
                        .to_string(),
                );
            }
            inside_text_input = false;
        }
    }

    let mut declared_ids = std::collections::HashSet::new();
    for line in body.lines() {
        let trimmed = line.trim();
        if let Some((before, _)) = trimmed.split_once(":=")
            && let Some(name) = before.split_whitespace().last()
            && name
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            declared_ids.insert(name.to_string());
        }
    }

    const PROPERTY_ROOTS: &[&str] = &[
        "align",
        "body",
        "content",
        "draw_bg",
        "draw_cursor",
        "draw_icon",
        "draw_selection",
        "draw_text",
        "header",
        "icon_walk",
        "label_align",
        "label_walk",
        "popup_menu",
        "scroll_bar",
        "scroll_bars",
        "walk",
        "window",
    ];

    for line in body.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("//") {
            continue;
        }

        let Some((before_colon, _)) = trimmed.split_once(':') else {
            continue;
        };
        let property_path = before_colon
            .split_whitespace()
            .last()
            .unwrap_or(before_colon)
            .rsplit('{')
            .next()
            .unwrap_or(before_colon)
            .trim();

        let Some((root, _)) = property_path.split_once('.') else {
            continue;
        };

        if PROPERTY_ROOTS.contains(&root) {
            continue;
        }

        if root
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
            && !declared_ids.contains(root)
        {
            return Some(format!(
                "it referenced named child `{}` without declaring `{}` with `:=` first",
                root, root
            ));
        }
    }

    None
}

pub fn start_infra(
    client: Arc<dyn InferenceClient>,
    runtime: AppRuntime,
) -> Box<dyn WorkspaceAgent> {
    let (doc_handle, task, chat_tx, listener_tasks) =
        start_automerge_infrastructure(client, runtime);
    let child = spawn_app_client(runtime);

    Box::new(AutomergeAgent {
        doc_handle,
        agent_task: Mutex::new(Some(task)),
        listener_tasks: Mutex::new(listener_tasks),
        web_child: Mutex::new(child),
        chat_tx,
    })
}

struct ChatRequest {
    content: String,
    model: Option<String>,
    responder: oneshot::Sender<Option<String>>,
}

struct AutomergeAgent {
    doc_handle: DocHandle,
    agent_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
    listener_tasks: Mutex<Vec<JoinHandle<()>>>,
    web_child: Mutex<Option<Child>>,
    chat_tx: mpsc::Sender<ChatRequest>,
}

/// Web sink used in the server process to enqueue web responses into the shared doc.
///
/// This does not call any web APIs directly; it only writes `AgentResponse` entries
/// that the web process will observe and handle.
struct DocAppSink {
    doc_handle: DocHandle,
}

/// Web agent used in the web client process to enqueue requests into the shared doc.
///
/// The web client cannot call the inference client directly, so it records
/// `AgentRequest` entries that the server process will consume.
pub struct DocWebAgent {
    doc_handle: DocHandle,
}

impl DocWebAgent {
    pub fn new(doc_handle: DocHandle) -> Self {
        Self { doc_handle }
    }
}

/// Starts the web backend loop in the web client process.
///
/// This connects to the shared document, watches for `AgentResponse` entries,
/// and forwards them to the provided `App` implementation (which owns the UI host).
/// It returns a `WebAgent` that writes requests into the shared document for the
/// server process to handle.
pub async fn start_app_backend(app: Arc<dyn App>) -> (Box<dyn WebAgent>, mpsc::Receiver<()>) {
    let doc_handle = setup_web_doc().await;
    let agent = DocWebAgent::new(doc_handle.clone());
    let (exit_tx, exit_rx) = mpsc::channel(1);

    eprintln!(
        "[LSP Agent Host] Connected to shared doc {} and waiting for app responses.",
        doc_handle.document_id()
    );

    tokio::spawn(async move {
        loop {
            if doc_handle.changed().await.is_err() {
                let _ = exit_tx.send(()).await;
                break;
            }

            if handle_web_doc_change(&doc_handle, app.as_ref()).await {
                let _ = exit_tx.send(()).await;
                break;
            }
        }
    });

    (Box::new(agent), exit_rx)
}

#[async_trait::async_trait]
impl App for DocAppSink {
    async fn launch_app(&self, id: String, content: String) {
        eprintln!(
            "[LSP Agent] Enqueueing app {} into shared doc ({} chars): {}",
            id,
            content.chars().count(),
            log_preview(&content)
        );
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.webviews.documents.insert(
                id.clone(),
                DocumentContent {
                    text: content.clone(),
                },
            );
            agent.responses.push(AgentResponse::WebApp { id, content });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn handle_inference_response(&self, app_id: String, content: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .responses
                .push(AgentResponse::Inference { app_id, content });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }
}

#[async_trait::async_trait]
impl WorkspaceAgent for AutomergeAgent {
    async fn shutdown(&self) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.should_exit = true;
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        let listener_tasks = {
            let mut tasks = self.listener_tasks.lock().await;
            std::mem::take(&mut *tasks)
        };

        for task in listener_tasks {
            task.abort();
            let _ = task.await;
        }

        if let Some(task) = self.agent_task.lock().await.take() {
            let _ = task.await;
        }

        if let Some(mut child) = self.web_child.lock().await.take() {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
    }

    async fn did_open(&self, uri: String, text: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .text_documents
                .documents
                .insert(uri, DocumentContent { text });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn did_change(&self, uri: String, text: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .text_documents
                .documents
                .insert(uri, DocumentContent { text });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn did_close(&self, uri: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.text_documents.documents.remove(&uri);
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn set_active_document(&self, uri: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.text_documents.active_document = Some(Uri { value: uri });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn chat_request(&self, content: String, model: Option<String>) -> Option<String> {
        let (tx, rx) = oneshot::channel();
        let req = ChatRequest {
            content,
            model,
            responder: tx,
        };
        if self.chat_tx.send(req).await.is_err() {
            return None;
        }
        rx.await.ok().flatten()
    }
}

#[async_trait::async_trait]
impl WebAgent for DocWebAgent {
    async fn app_inference_request(&self, content: String, app_id: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .requests
                .push(AgentRequest::Inference { content, app_id });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn read_document(&self, uri: String) -> String {
        self.doc_handle.with_doc(|doc| {
            let agent: LspAgent = hydrate(doc).unwrap();
            agent
                .text_documents
                .documents
                .get(&uri)
                .map(|doc| doc.text.clone())
                .unwrap_or_default()
        })
    }

    async fn close_app(&self, app_id: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent.webviews.documents.remove(&app_id);
            agent
                .conversation_history
                .push(ConversationFragment::Assistant(format!(
                    "App closed: {}",
                    app_id
                )));
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn store_value(&self, key: String, value: String, description: String) {
        self.doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = hydrate(doc).unwrap();
            agent
                .stored_values
                .insert(key, StoredValue { value, description });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });
    }

    async fn read_value(&self, key: String) -> Option<String> {
        self.doc_handle.with_doc(|doc| {
            let agent: LspAgent = hydrate(doc).unwrap();
            agent.stored_values.get(&key).map(|v| v.value.clone())
        })
    }
}

async fn setup_web_doc() -> DocHandle {
    let repo = Repo::new(None, Box::new(NoStorage));
    let repo_handle = repo.run();
    connect_to_peer1(repo_handle.clone());

    let doc_id = wait_for_doc_id().await;
    println!("Found Doc ID: {}", doc_id);

    repo_handle.request_document(doc_id.clone()).await.unwrap()
}

fn connect_to_peer1(repo_handle: RepoHandle) {
    tokio::spawn(async move {
        let addr = format!("127.0.0.1:{}", PEER1_PORT);
        loop {
            match TcpStream::connect(&addr).await {
                Ok(stream) => {
                    repo_handle
                        .connect_tokio_io(addr.clone(), stream, ConnDirection::Outgoing)
                        .await
                        .unwrap();
                    break;
                }
                Err(_) => {
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    });
}

async fn wait_for_doc_id() -> DocumentId {
    println!("Waiting for doc_id from HTTP...");
    let doc_id_str = loop {
        match reqwest::get("http://127.0.0.1:2348/doc_id").await {
            Ok(resp) => {
                if let Ok(text) = resp.text().await {
                    break text.trim().to_string();
                }
            }
            Err(_) => {
                sleep(Duration::from_millis(1000)).await;
            }
        }
    };

    doc_id_str.parse().expect("Failed to parse document ID")
}

async fn handle_web_doc_change(doc_handle: &DocHandle, app: &dyn App) -> bool {
    let (should_exit, should_handle_response) = doc_handle.with_doc(|doc| {
        let agent: LspAgent = hydrate(doc).unwrap();
        let handle = match agent.responses.first() {
            Some(AgentResponse::Chat(_)) => false,
            Some(_) => true,
            None => false,
        };
        (agent.should_exit, handle)
    });

    if should_exit {
        return true;
    }

    if should_handle_response {
        let response_enum = take_response(doc_handle);
        if let Some(resp) = response_enum {
            handle_web_response(app, resp).await;
        }
    }

    false
}

fn take_response(doc_handle: &DocHandle) -> Option<AgentResponse> {
    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = match hydrate(doc) {
            Ok(a) => a,
            Err(_) => return None,
        };
        let resp = if !agent.responses.is_empty() {
            Some(agent.responses.remove(0))
        } else {
            None
        };

        if resp.is_some() {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        }

        resp
    })
}

async fn handle_web_response(app: &dyn App, resp: AgentResponse) {
    match resp {
        AgentResponse::WebApp { id, content } => {
            eprintln!(
                "[LSP Agent Host] Consuming WebApp response for {} ({} chars): {}",
                id,
                content.chars().count(),
                log_preview(&content)
            );
            app.launch_app(id, content).await;
        }
        AgentResponse::Chat(_) => {
            debug_assert!(false, "Web backend should not consume chat responses");
        }
        AgentResponse::Inference { app_id, content } => {
            eprintln!(
                "[LSP Agent Host] Consuming inference response for {} ({} chars): {}",
                app_id,
                content.chars().count(),
                log_preview(&content)
            );
            app.handle_inference_response(app_id, content).await;
        }
    }
}

fn spawn_app_client(runtime: AppRuntime) -> Option<Child> {
    let exe_path = std::env::current_exe().expect("Failed to get current exe path");
    let project_root = find_repo_root(&exe_path).unwrap_or_else(|| {
        exe_path
            .parent()
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .and_then(|p| p.parent())
            .expect("Failed to find project root")
            .to_path_buf()
    });

    let app_binary = match runtime
        .candidate_binaries(&project_root)
        .into_iter()
        .find(|path| path.exists())
    {
        Some(path) => path,
        None => {
            eprintln!(
                "[LSP Agent] {} binary not found. Rebuild the matching crate or set LSP_AGENT_APP_BINARY.",
                runtime.host_label(),
            );
            return None;
        }
    };

    let mut child = Command::new(&app_binary);
    child.env(
        "RUST_BACKTRACE",
        std::env::var("RUST_BACKTRACE").unwrap_or_else(|_| "1".to_string()),
    );
    child.env(
        "RUST_LIB_BACKTRACE",
        std::env::var("RUST_LIB_BACKTRACE").unwrap_or_else(|_| "1".to_string()),
    );
    child.stdout(std::process::Stdio::null());
    child.stderr(std::process::Stdio::inherit());
    match child.spawn() {
        Ok(child) => {
            eprintln!(
                "[LSP Agent] Spawned {} at {} with pid {}.",
                runtime.host_label(),
                app_binary.display(),
                child.id().unwrap_or_default()
            );
            Some(child)
        }
        Err(err) => {
            eprintln!(
                "[LSP Agent] Failed to spawn {} at {}: {:?}",
                runtime.host_label(),
                app_binary.display(),
                err
            );
            None
        }
    }
}

/// Starts the server-side infrastructure in the LSP/server process.
///
/// This owns the inference client, consumes `AgentRequest` entries from the shared doc,
/// and writes `AgentResponse` entries that the web client will handle.
fn start_automerge_infrastructure(
    client: Arc<dyn InferenceClient>,
    runtime: AppRuntime,
) -> (
    DocHandle,
    tokio::task::JoinHandle<()>,
    mpsc::Sender<ChatRequest>,
    Vec<JoinHandle<()>>,
) {
    let handle = Handle::current();

    let repo1 = Repo::new(None, Box::new(NoStorage));
    let repo_handle1 = repo1.run();

    let doc_handle = repo_handle1.new_document();
    let doc_id = doc_handle.document_id();

    let doc_http_listener = bind_local_listener(DOC_ID_PORT, "doc_id HTTP listener");
    let peer_listener = bind_local_listener(PEER1_PORT, "Makepad peer listener");

    let doc_id_str = doc_id.to_string();
    let doc_id_server_task = tokio::spawn(async move {
        let app = axum::Router::new().route(
            "/doc_id",
            axum::routing::get(move || async move { doc_id_str }),
        );
        let listener = tokio::net::TcpListener::from_std(doc_http_listener)
            .expect("failed to convert doc_id listener to tokio listener");
        eprintln!(
            "[LSP Agent] Serving doc_id {} on 127.0.0.1:{}.",
            doc_id, DOC_ID_PORT
        );
        if let Err(error) = axum::serve(listener, app).await {
            eprintln!("[LSP Agent] doc_id HTTP server stopped: {:?}", error);
        }
    });

    let peer_listener_task = spawn_peer_connections(repo_handle1.clone(), peer_listener);

    doc_handle.with_doc_mut(|doc| {
        let mut tx = doc.transaction();
        let agent = LspAgent::default();
        reconcile(&mut tx, &agent).unwrap();
        tx.commit();
    });

    let main_task_doc_handle = doc_handle.clone();
    let app_sink: Arc<dyn App> = Arc::new(DocAppSink {
        doc_handle: doc_handle.clone(),
    });
    let main_task_repo_handle = repo_handle1.clone();
    let main_task_client = client.clone();

    let (chat_tx, mut chat_rx) = mpsc::channel::<ChatRequest>(32);
    let main_task = handle.spawn(async move {
        loop {
            tokio::select! {
                changed = main_task_doc_handle.changed() => {
                    if changed.is_err() {
                        break;
                    }

                    let (should_exit, pending_request, active_model) = check_agent_state(&main_task_doc_handle);

                    if should_exit {
                        perform_shutdown(&main_task_client, &main_task_repo_handle).await;
                        break;
                    }

                    if let Some(req) = pending_request {
                        handle_inference_request(req, &main_task_client, active_model, app_sink.as_ref()).await;
                    }
                }
                Some(chat_req) = chat_rx.recv() => {
                    handle_chat_request(chat_req, runtime, &main_task_doc_handle, &main_task_client, app_sink.as_ref()).await;
                }
                else => {
                    break;
                }
            }
        }
    });

    (
        doc_handle,
        main_task,
        chat_tx,
        vec![doc_id_server_task, peer_listener_task],
    )
}

fn check_agent_state(doc_handle: &DocHandle) -> (bool, Option<AgentRequest>, Option<String>) {
    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = hydrate(doc).unwrap();
        let req = if !agent.requests.is_empty() {
            Some(agent.requests.remove(0))
        } else {
            None
        };

        if req.is_some() {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        }

        (agent.should_exit, req, agent.active_model)
    })
}

async fn perform_shutdown(client: &Arc<dyn InferenceClient>, repo_handle: &RepoHandle) {
    client.notify_shutdown().await;
    let repo_handle = repo_handle.clone();
    Handle::current()
        .spawn_blocking(move || {
            repo_handle.stop().unwrap();
        })
        .await
        .unwrap();
}

async fn handle_inference_request(
    req: AgentRequest,
    client: &Arc<dyn InferenceClient>,
    active_model: Option<String>,
    app_sink: &dyn App,
) {
    match req {
        AgentRequest::Inference { content, app_id } => {
            let response_str = call_inference(client.as_ref(), content, active_model).await;
            app_sink
                .handle_inference_response(app_id, response_str)
                .await;
        }
    }
}

fn spawn_peer_connections(repo_handle: RepoHandle, listener: StdTcpListener) -> JoinHandle<()> {
    let repo_clone1 = repo_handle.clone();
    tokio::spawn(async move {
        let listener = TcpListener::from_std(listener)
            .expect("failed to convert Makepad peer listener to tokio listener");
        eprintln!(
            "[LSP Agent] Listening for Makepad peer connections on 127.0.0.1:{}.",
            PEER1_PORT
        );
        loop {
            match listener.accept().await {
                Ok((socket, addr)) => {
                    eprintln!(
                        "[LSP Agent] Accepted Makepad peer connection from {}.",
                        addr
                    );
                    if let Err(error) = repo_clone1
                        .connect_tokio_io(addr, socket, ConnDirection::Incoming)
                        .await
                    {
                        eprintln!(
                            "[LSP Agent] Failed to attach Makepad peer connection from {}: {:?}",
                            addr, error
                        );
                    }
                }
                Err(error) => {
                    eprintln!(
                        "[LSP Agent] Makepad peer listener stopped on 127.0.0.1:{}: {:?}",
                        PEER1_PORT, error
                    );
                    break;
                }
            }
        }
    })
}

async fn handle_chat_request(
    chat_req: ChatRequest,
    runtime: AppRuntime,
    doc_handle: &DocHandle,
    client: &Arc<dyn InferenceClient>,
    app_sink: &dyn App,
) {
    let ChatRequest {
        content: latest_user,
        model: model_hint,
        responder,
    } = chat_req;

    let (mut history, running_apps, docs_info, stored_values_info) = doc_handle.with_doc(|doc| {
        let agent: LspAgent = hydrate(doc).unwrap();
        (
            agent.conversation_history.clone(),
            collect_apps(&agent.webviews),
            collect_docs(&agent.text_documents),
            collect_stored_values(&agent.stored_values),
        )
    });

    let initial_history_len = history.len();

    let mut apps_payload: Option<Vec<String>> = None;
    let mut docs_payload: Option<prompts::DocsInfo> = None;
    let mut stored_values_payload: Option<Vec<prompts::StoredValueInfo>> = None;
    let standard_apps_payload = standard_makepad_app_infos(runtime);
    let mut response_message: Option<String> = None;
    let mut launched_app: Option<String> = None;
    let mut did_nothing = false;

    let mut current_prompt_user = latest_user.clone();
    let mut pushed_user_message = false;

    let max_iterations = std::env::var("LSP_AGENT_TOOL_MAX_ITERATIONS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(DEFAULT_TOOL_MAX_ITERATIONS);

    for _ in 0..max_iterations {
        let request_text = prompts::build_app_request(
            runtime.as_str(),
            &history,
            &current_prompt_user,
            apps_payload.as_deref(),
            docs_payload.as_ref(),
            stored_values_payload.as_deref(),
            standard_apps_payload.as_deref(),
        );
        let tool_response_str =
            call_inference(client.as_ref(), request_text, model_hint.clone()).await;
        let tool_response = parse_tool_response(&tool_response_str);

        let mut next_turn_reason: Option<String> = None;

        match tool_response.action.as_str() {
            "answer" => {
                response_message = tool_response.message;
                break;
            }
            "nothing" => {
                did_nothing = true;
                break;
            }
            "launch_app" => {
                let Some(app_body) = tool_response.app else {
                    response_message =
                        Some("The assistant requested launch_app without an app body.".to_string());
                    break;
                };

                if runtime == AppRuntime::Makepad
                    && let Some(reason) = validate_makepad_splash_body(&app_body)
                {
                    eprintln!(
                        "[LSP Agent] Rejecting generated Makepad app before launch: {} ({} chars): {}",
                        reason,
                        app_body.chars().count(),
                        log_preview(&app_body)
                    );
                    next_turn_reason = Some(format!(
                        "The previous Splash app could not be launched because {}. Generate a corrected embedded mini app body that follows the runtime rules.",
                        reason
                    ));
                } else {
                    launched_app = Some(app_body);
                    break;
                }
            }
            "launch_standard_app" => {
                let Some(app_id) = tool_response.standard_app_id.as_deref() else {
                    response_message = Some(
                        "The assistant requested launch_standard_app without a standard_app_id."
                            .to_string(),
                    );
                    break;
                };

                match standard_makepad_app_by_id(runtime, app_id) {
                    Some(app) => {
                        eprintln!(
                            "[LSP Agent] Using standard Makepad {} app template for request {:?} ({} chars): {}",
                            app.id,
                            latest_user,
                            app.content.chars().count(),
                            log_preview(app.content)
                        );
                        launched_app = Some(app.content.to_string());
                    }
                    None => {
                        response_message = Some(format!(
                            "Unknown standard app '{}'. Please choose one of the advertised standard_app ids or generate a custom app.",
                            app_id
                        ));
                    }
                }
                break;
            }
            "list_apps" => {
                if apps_payload.is_some() {
                    response_message = Some(
                        "App list was already provided, but the assistant requested it again without concluding."
                            .to_string(),
                    );
                    break;
                }
                apps_payload = Some(running_apps.clone());
                next_turn_reason = Some("Assistant requested info on running apps.".to_string());
            }
            "list_docs" => {
                if docs_payload.is_some() {
                    response_message = Some(
                        "Document list was already provided, but the assistant requested it again without concluding."
                            .to_string(),
                    );
                    break;
                }
                docs_payload = Some(docs_info.clone());
                next_turn_reason = Some("Assistant requested info on open documents.".to_string());
            }
            "list_app_values" => {
                if stored_values_payload.is_some() {
                    response_message = Some(
                        "Stored values list was already provided, but the assistant requested it again without concluding."
                            .to_string(),
                    );
                    break;
                }
                stored_values_payload = Some(stored_values_info.clone());
                next_turn_reason = Some("Assistant requested info on stored values.".to_string());
            }
            _ => {
                response_message = Some(tool_response_str);
            }
        }

        if let Some(reason) = next_turn_reason {
            if !pushed_user_message && !current_prompt_user.is_empty() {
                history.push(ConversationFragment::User(current_prompt_user.clone()));
                current_prompt_user.clear();
                pushed_user_message = true;
            }
            history.push(ConversationFragment::Assistant(reason));
            continue;
        }
    }

    if !did_nothing && launched_app.is_none() && response_message.is_none() {
        response_message =
            Some("No actionable response was produced. Please retry or rephrase.".to_string());
    }

    let launched_app_for_doc = launched_app.clone();
    let did_launch_app = launched_app.is_some();
    // did_request_docs and did_request_apps removed as we use history diff

    doc_handle.with_doc_mut(|doc| {
        let mut agent: LspAgent = hydrate(doc).unwrap();
        if let Some(model) = model_hint.clone() {
            agent.active_model = Some(model);
        }

        // 1. Add any history accumulated during tool use (User messages + Assistant markers)
        let new_fragments: Vec<ConversationFragment> =
            history.iter().skip(initial_history_len).cloned().collect();
        agent.conversation_history.extend(new_fragments);

        // 2. Ensuring user message is present if not already in history (e.g. immediate answer/launch)
        if !pushed_user_message
            && !latest_user.is_empty()
            && (did_launch_app || response_message.is_some())
        {
            agent
                .conversation_history
                .push(ConversationFragment::User(latest_user.clone()));
        }

        // 3. Add final response
        if let Some(message) = response_message.clone() {
            agent
                .conversation_history
                .push(ConversationFragment::Assistant(message));
        }

        let mut tx = doc.transaction();
        reconcile(&mut tx, &agent).unwrap();
        tx.commit();
    });

    if let Some(app) = launched_app_for_doc {
        let app_id = format!("app-{}", Uuid::new_v4());
        app_sink.launch_app(app_id.clone(), app.clone()).await;
    }

    if let Some(message) = response_message {
        let _ = responder.send(Some(message));
    } else {
        let _ = responder.send(None);
    }
}

async fn call_inference(
    client: &dyn InferenceClient,
    request: String,
    model: Option<String>,
) -> String {
    match client.inference(request, model).await {
        Ok(res) => res,
        Err(e) => format!("Error: {}", e),
    }
}

fn parse_tool_response(response: &str) -> ToolResponse {
    match serde_json::from_str::<ToolResponse>(response) {
        Ok(mut parsed) => {
            if parsed.action == "answer" && parsed.message.is_none() {
                parsed.message = Some(response.to_string());
            }
            parsed
        }
        Err(_) => ToolResponse {
            action: "answer".to_string(),
            message: Some(response.to_string()),
            app: None,
            standard_app_id: None,
        },
    }
}

fn collect_apps(manager: &DocumentManager) -> Vec<String> {
    manager
        .documents
        .values()
        .map(|doc| doc.text.clone())
        .collect()
}

fn collect_docs(manager: &DocumentManager) -> prompts::DocsInfo {
    let mut open_documents: Vec<String> = manager.documents.keys().cloned().collect();
    open_documents.sort();
    let active_document = manager
        .active_document
        .as_ref()
        .map(|uri| uri.value.clone());
    if let Some(active) = &active_document
        && !open_documents.contains(active)
    {
        open_documents.push(active.clone());
    }
    prompts::DocsInfo {
        open_documents,
        active_document,
    }
}

fn collect_stored_values(
    values: &std::collections::HashMap<String, StoredValue>,
) -> Vec<prompts::StoredValueInfo> {
    values
        .iter()
        .map(|(k, v)| prompts::StoredValueInfo {
            key: k.clone(),
            description: v.description.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{DocumentManager, StoredValue, Uri};

    #[test]
    fn test_find_repo_root_with_workspace() {
        use std::fs::File;
        use std::io::Write;
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let cargo_toml_path = temp_dir.path().join("Cargo.toml");

        let mut file = File::create(&cargo_toml_path).unwrap();
        writeln!(file, "[workspace]").unwrap();

        let exe_path = temp_dir.path().join("bin").join("test_exe");
        std::fs::create_dir_all(exe_path.parent().unwrap()).unwrap();

        let result = find_repo_root(&exe_path);
        assert_eq!(result, Some(temp_dir.path().to_path_buf()));
    }

    #[test]
    fn test_find_repo_root_without_workspace() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let exe_path = temp_dir.path().join("test_exe");

        let result = find_repo_root(&exe_path);
        assert_eq!(result, None);
    }

    #[test]
    fn test_collect_apps() {
        let mut manager = DocumentManager::default();
        manager.documents.insert(
            "app1".to_string(),
            DocumentContent {
                text: "html1".to_string(),
            },
        );
        manager.documents.insert(
            "app2".to_string(),
            DocumentContent {
                text: "html2".to_string(),
            },
        );

        let apps = collect_apps(&manager);
        assert_eq!(apps.len(), 2);
        assert!(apps.contains(&"html1".to_string()));
        assert!(apps.contains(&"html2".to_string()));
    }

    #[test]
    fn test_collect_docs_basic() {
        let mut manager = DocumentManager::default();
        manager.documents.insert(
            "file1.rs".to_string(),
            DocumentContent {
                text: "code1".to_string(),
            },
        );
        manager.documents.insert(
            "file2.rs".to_string(),
            DocumentContent {
                text: "code2".to_string(),
            },
        );

        let docs = collect_docs(&manager);
        assert_eq!(docs.open_documents.len(), 2);
        assert!(docs.open_documents.contains(&"file1.rs".to_string()));
        assert!(docs.open_documents.contains(&"file2.rs".to_string()));
        assert_eq!(docs.active_document, None);
    }

    #[test]
    fn test_collect_docs_with_active() {
        let mut manager = DocumentManager::default();
        manager.documents.insert(
            "file1.rs".to_string(),
            DocumentContent {
                text: "code1".to_string(),
            },
        );
        manager.active_document = Some(Uri {
            value: "file1.rs".to_string(),
        });

        let docs = collect_docs(&manager);
        assert_eq!(docs.open_documents.len(), 1);
        assert_eq!(docs.active_document, Some("file1.rs".to_string()));
    }

    #[test]
    fn test_collect_docs_active_not_in_open() {
        let mut manager = DocumentManager::default();
        manager.documents.insert(
            "file1.rs".to_string(),
            DocumentContent {
                text: "code1".to_string(),
            },
        );
        manager.active_document = Some(Uri {
            value: "file2.rs".to_string(),
        });

        let docs = collect_docs(&manager);
        assert_eq!(docs.open_documents.len(), 2);
        assert!(docs.open_documents.contains(&"file1.rs".to_string()));
        assert!(docs.open_documents.contains(&"file2.rs".to_string()));
        assert_eq!(docs.active_document, Some("file2.rs".to_string()));
    }

    #[test]
    fn test_collect_stored_values() {
        let mut values = std::collections::HashMap::new();
        values.insert(
            "key1".to_string(),
            StoredValue {
                value: "value1".to_string(),
                description: "desc1".to_string(),
            },
        );
        values.insert(
            "key2".to_string(),
            StoredValue {
                value: "value2".to_string(),
                description: "desc2".to_string(),
            },
        );

        let infos = collect_stored_values(&values);
        assert_eq!(infos.len(), 2);

        let info1 = infos.iter().find(|i| i.key == "key1").unwrap();
        assert_eq!(info1.description, "desc1");

        let info2 = infos.iter().find(|i| i.key == "key2").unwrap();
        assert_eq!(info2.description, "desc2");
    }

    #[test]
    fn test_parse_tool_response_valid_json() {
        let json = r#"{"action": "answer", "message": "Hello!", "app": "test"}"#;
        let response = parse_tool_response(json);

        assert_eq!(response.action, "answer");
        assert_eq!(response.message, Some("Hello!".to_string()));
        assert_eq!(response.app, Some("test".to_string()));
    }

    #[test]
    fn test_parse_tool_response_answer_without_message() {
        let json = r#"{"action": "answer"}"#;
        let response = parse_tool_response(json);

        assert_eq!(response.action, "answer");
        assert_eq!(
            response.message,
            Some(r#"{"action": "answer"}"#.to_string())
        );
    }

    #[test]
    fn test_parse_tool_response_invalid_json() {
        let invalid_json = "not json";
        let response = parse_tool_response(invalid_json);

        assert_eq!(response.action, "answer");
        assert_eq!(response.message, Some("not json".to_string()));
        assert_eq!(response.app, None);
    }

    #[test]
    fn test_standard_makepad_app_by_id_returns_todo_template() {
        let app =
            standard_makepad_app_by_id(AppRuntime::Makepad, "todo").expect("expected todo app");

        assert_eq!(app.id, "todo");
        assert!(app.content.contains("let todos = ["));
        assert!(app.content.contains("fn sync_rows(){"));
        assert!(app.content.contains("todo_input := TextInput{"));
        assert!(!app.content.contains("on_render:"));
        assert!(validate_makepad_splash_body(app.content).is_none());
    }

    #[test]
    fn test_standard_makepad_app_by_id_returns_notes_template() {
        let app =
            standard_makepad_app_by_id(AppRuntime::Makepad, "notes").expect("expected notes app");

        assert_eq!(app.id, "notes");
        assert!(app.content.contains("let notes = ["));
        assert!(app.content.contains("fn sync_rows(){"));
        assert!(app.content.contains("note_input := TextInput{"));
        assert!(!app.content.contains("on_render:"));
        assert!(validate_makepad_splash_body(app.content).is_none());
    }

    #[test]
    fn test_standard_makepad_app_by_id_rejects_unknown_or_wrong_runtime() {
        assert!(standard_makepad_app_by_id(AppRuntime::Web, "todo").is_none());
        assert!(standard_makepad_app_by_id(AppRuntime::Makepad, "unknown").is_none());
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_parenthesized_if() {
        let reason = validate_makepad_splash_body(
            r#"RoundedView{
    width: Fill height: Fit
    Label{text: "Hi"}
    on_click: || { if (1 == 1) { ui.title.set_text("ok") } }
}"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("parenthesized `if` conditions"));
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_root_on_render_initializer() {
        let reason = validate_makepad_splash_body(
            r#"RoundedView{
    width: Fill height: Fit
    Label{text: "Hi"}
    on_render: ||{ ui.title.set_text("ok") }
}"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("used `on_render`"));
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_nested_on_render() {
        let reason = validate_makepad_splash_body(
            r#"RoundedView{
    width: Fill height: Fit
    list := View{
        width: Fill height: Fit
        on_render: ||{
            Label{text: "Nested"}
        }
    }
}"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("used `on_render`"));
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_text_input_fit_height() {
        let reason = validate_makepad_splash_body(
            r#"RoundedView{
    width: Fill height: Fit
    note_input := TextInput{
        width: Fill height: Fit
        empty_text: "Write something"
    }
}"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("TextInput` with `height: Fit"));
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_text_input_without_height() {
        let reason = validate_makepad_splash_body(
            r#"RoundedView{
    width: Fill height: Fit
    note_input := TextInput{
        width: Fill
        empty_text: "Write something"
    }
}"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("without an explicit fixed height"));
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_top_level_initializer_call() {
        let reason = validate_makepad_splash_body(
            r#"let rows = ["A"]

fn sync_rows(){
    ui.row_0.set_text(rows[0])
}

RoundedView{
    width: Fill height: Fit
    row_0 := Label{text: "A"}
}

sync_rows()"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("top-level initialization code like `sync_rows()`"));
    }

    #[test]
    fn test_validate_makepad_splash_body_rejects_missing_named_child_declaration() {
        let reason = validate_makepad_splash_body(
            r#"let Row = RoundedView{
    width: Fill height: Fit
    Label{text: "task"}
}

RoundedView{
    width: Fill height: Fit
    Row{label.text: "Broken"}
}"#,
        )
        .expect("expected validation failure");

        assert!(reason.contains("without declaring `label`"));
    }

    #[test]
    fn test_validate_makepad_splash_body_allows_declared_named_child_override() {
        assert!(
            validate_makepad_splash_body(
                r#"let Row = RoundedView{
    width: Fill height: Fit
    label := Label{text: "task"}
}

RoundedView{
    width: Fill height: Fit
    Row{label.text: "Works"}
}"#,
            )
            .is_none()
        );
    }

    #[tokio::test]
    async fn test_call_inference_success() {
        use async_trait::async_trait;
        use mockall::mock;
        use traits::InferenceClient;

        mock! {
            pub TestClient {}
            #[async_trait]
            impl InferenceClient for TestClient {
                async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
                async fn notify_shutdown(&self);
            }
        }

        let mut mock_client = MockTestClient::new();
        mock_client
            .expect_inference()
            .returning(|_, _| Ok("response".to_string()));

        let result = call_inference(&mock_client, "request".to_string(), None).await;
        assert_eq!(result, "response");
    }

    #[tokio::test]
    async fn test_call_inference_error() {
        use async_trait::async_trait;
        use mockall::mock;
        use traits::InferenceClient;

        mock! {
            pub TestClient {}
            #[async_trait]
            impl InferenceClient for TestClient {
                async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
                async fn notify_shutdown(&self);
            }
        }

        let mut mock_client = MockTestClient::new();
        mock_client
            .expect_inference()
            .returning(|_, _| Err("inference error".to_string()));

        let result = call_inference(&mock_client, "request".to_string(), None).await;
        assert_eq!(result, "Error: inference error");
    }

    // Web response handling tests
    #[tokio::test]
    async fn test_handle_web_doc_change_launch_app() {
        struct RecordingWeb {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
            inference: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingWeb {
            async fn launch_app(&self, id: String, content: String) {
                let mut l = self.launched.lock().await;
                l.push((id, content));
            }

            async fn handle_inference_response(&self, app_id: String, content: String) {
                let mut v = self.inference.lock().await;
                v.push((app_id, content));
            }
        }

        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        // Insert a WebApp response
        doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            agent.responses.push(AgentResponse::WebApp {
                id: "appA".to_string(),
                content: "<html/>".to_string(),
            });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        let web = RecordingWeb {
            launched: tokio::sync::Mutex::new(vec![]),
            inference: tokio::sync::Mutex::new(vec![]),
        };
        let rc = &web;

        // Should handle and launch app
        let handled = handle_web_doc_change(&doc_handle, rc).await;
        assert!(!handled, "should not signal exit");

        // verify launched
        let launched = web.launched.lock().await;
        assert_eq!(launched.len(), 1);
        assert_eq!(launched[0].0, "appA");
        assert_eq!(launched[0].1, "<html/>".to_string());

        // ensure response removed
        doc_handle.with_doc(|doc| {
            let agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            assert!(agent.responses.is_empty());
        });
    }

    #[tokio::test]
    async fn test_handle_web_doc_change_inference() {
        struct RecordingWeb {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
            inference: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingWeb {
            async fn launch_app(&self, _id: String, _content: String) {
                // no-op for this test
            }

            async fn handle_inference_response(&self, app_id: String, content: String) {
                let mut v = self.inference.lock().await;
                v.push((app_id, content));
            }
        }

        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            agent.responses.push(AgentResponse::Inference {
                app_id: "a1".to_string(),
                content: "ok".to_string(),
            });
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        let web = RecordingWeb {
            launched: tokio::sync::Mutex::new(vec![]),
            inference: tokio::sync::Mutex::new(vec![]),
        };
        let rc = &web;

        let handled = handle_web_doc_change(&doc_handle, rc).await;
        assert!(!handled);

        let inf = web.inference.lock().await;
        assert_eq!(inf.len(), 1);
        assert_eq!(inf[0].0, "a1");
        assert_eq!(inf[0].1, "ok".to_string());

        doc_handle.with_doc(|doc| {
            let agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            assert!(agent.responses.is_empty());
        });
    }

    #[tokio::test]
    async fn test_handle_web_doc_change_chat_ignored() {
        struct RecordingWeb {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
            inference: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingWeb {
            async fn launch_app(&self, _id: String, _content: String) {}
            async fn handle_inference_response(&self, _app_id: String, _content: String) {}
        }

        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            agent.responses.push(AgentResponse::Chat("hey".to_string()));
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        let web = RecordingWeb {
            launched: tokio::sync::Mutex::new(vec![]),
            inference: tokio::sync::Mutex::new(vec![]),
        };
        let rc = &web;

        let handled = handle_web_doc_change(&doc_handle, rc).await;
        assert!(!handled);

        // Chat response should still be present because it is ignored by web handler
        doc_handle.with_doc(|doc| {
            let agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            assert_eq!(agent.responses.len(), 1);
            match &agent.responses[0] {
                AgentResponse::Chat(msg) => assert_eq!(msg, "hey"),
                _ => panic!("expected chat"),
            }
        });
    }

    #[tokio::test]
    async fn test_take_response_direct_and_should_exit() {
        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        // take_response returns None when empty
        let none = take_response(&doc_handle);
        assert!(none.is_none());

        // add a response and ensure take_response returns it and removes it
        doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            agent
                .responses
                .push(AgentResponse::Chat("hello".to_string()));
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        let resp = take_response(&doc_handle);
        assert!(matches!(resp, Some(AgentResponse::Chat(_))));

        // should_exit test
        doc_handle.with_doc_mut(|doc| {
            let mut agent: LspAgent = match hydrate(doc) {
                Ok(a) => a,
                Err(_) => LspAgent::default(),
            };
            agent.should_exit = true;
            let mut tx = doc.transaction();
            reconcile(&mut tx, &agent).unwrap();
            tx.commit();
        });

        struct RecordingWeb2 {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
            inference: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingWeb2 {
            async fn launch_app(&self, _id: String, _content: String) {}
            async fn handle_inference_response(&self, _app_id: String, _content: String) {}
        }

        let rc = RecordingWeb2 {
            launched: tokio::sync::Mutex::new(vec![]),
            inference: tokio::sync::Mutex::new(vec![]),
        };
        let ret = handle_web_doc_change(&doc_handle, &rc).await;
        assert!(ret, "expected true when should_exit is set");
    }

    #[tokio::test]
    async fn test_handle_chat_request_launches_standard_makepad_todo_app() {
        use async_trait::async_trait;
        use mockall::mock;

        mock! {
            pub StandardAppClient {}
            #[async_trait]
            impl InferenceClient for StandardAppClient {
                async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
                async fn notify_shutdown(&self);
            }
        }

        struct RecordingApp {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingApp {
            async fn launch_app(&self, id: String, content: String) {
                self.launched.lock().await.push((id, content));
            }

            async fn handle_inference_response(&self, _app_id: String, _content: String) {}
        }

        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &LspAgent::default()).unwrap();
            tx.commit();
        });

        let mut mock_client = MockStandardAppClient::new();
        mock_client
            .expect_inference()
            .times(1)
            .returning(|request, _| {
                assert!(request.contains("\"standard_apps\""));
                assert!(request.contains("\"id\": \"todo\""));
                Ok(r#"{"action":"launch_standard_app","standard_app_id":"todo"}"#.to_string())
            });
        mock_client.expect_notify_shutdown().times(0);

        let client: Arc<dyn InferenceClient> = Arc::new(mock_client);
        let app = RecordingApp {
            launched: tokio::sync::Mutex::new(vec![]),
        };

        let (tx, rx) = oneshot::channel();
        handle_chat_request(
            ChatRequest {
                content: "todo app".to_string(),
                model: Some("gpt-5.4".to_string()),
                responder: tx,
            },
            AppRuntime::Makepad,
            &doc_handle,
            &client,
            &app,
        )
        .await;

        assert_eq!(rx.await.unwrap(), None);

        let launched = app.launched.lock().await;
        assert_eq!(launched.len(), 1);
        assert!(launched[0].0.starts_with("app-"));
        assert!(launched[0].1.contains("fn sync_rows(){"));
        assert!(
            launched[0]
                .1
                .contains("ButtonFlatter{text: \"Clear completed\"")
        );
    }

    #[tokio::test]
    async fn test_handle_chat_request_launches_standard_makepad_notes_app() {
        use async_trait::async_trait;
        use mockall::mock;

        mock! {
            pub StandardAppClient {}
            #[async_trait]
            impl InferenceClient for StandardAppClient {
                async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
                async fn notify_shutdown(&self);
            }
        }

        struct RecordingApp {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingApp {
            async fn launch_app(&self, id: String, content: String) {
                self.launched.lock().await.push((id, content));
            }

            async fn handle_inference_response(&self, _app_id: String, _content: String) {}
        }

        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &LspAgent::default()).unwrap();
            tx.commit();
        });

        let mut mock_client = MockStandardAppClient::new();
        mock_client
            .expect_inference()
            .times(1)
            .returning(|request, _| {
                assert!(request.contains("\"standard_apps\""));
                assert!(request.contains("\"id\": \"notes\""));
                Ok(r#"{"action":"launch_standard_app","standard_app_id":"notes"}"#.to_string())
            });
        mock_client.expect_notify_shutdown().times(0);

        let client: Arc<dyn InferenceClient> = Arc::new(mock_client);
        let app = RecordingApp {
            launched: tokio::sync::Mutex::new(vec![]),
        };

        let (tx, rx) = oneshot::channel();
        handle_chat_request(
            ChatRequest {
                content: "app to write down stuff".to_string(),
                model: Some("gpt-5.4".to_string()),
                responder: tx,
            },
            AppRuntime::Makepad,
            &doc_handle,
            &client,
            &app,
        )
        .await;

        assert_eq!(rx.await.unwrap(), None);

        let launched = app.launched.lock().await;
        assert_eq!(launched.len(), 1);
        assert!(launched[0].0.starts_with("app-"));
        assert!(launched[0].1.contains("let notes = ["));
        assert!(launched[0].1.contains("fn sync_rows(){"));
        assert!(launched[0].1.contains("ButtonFlatter{text: \"Clear all\""));
    }

    #[tokio::test]
    async fn test_handle_chat_request_retries_invalid_makepad_launch_app() {
        use async_trait::async_trait;
        use mockall::mock;
        use std::sync::atomic::{AtomicUsize, Ordering};

        mock! {
            pub RetryingClient {}
            #[async_trait]
            impl InferenceClient for RetryingClient {
                async fn inference(&self, request: String, model: Option<String>) -> Result<String, String>;
                async fn notify_shutdown(&self);
            }
        }

        struct RecordingApp {
            launched: tokio::sync::Mutex<Vec<(String, String)>>,
        }

        #[async_trait::async_trait]
        impl App for RecordingApp {
            async fn launch_app(&self, id: String, content: String) {
                self.launched.lock().await.push((id, content));
            }

            async fn handle_inference_response(&self, _app_id: String, _content: String) {}
        }

        let repo = Repo::new(None, Box::new(NoStorage));
        let repo_handle = repo.run();
        let doc_handle = repo_handle.new_document();

        doc_handle.with_doc_mut(|doc| {
            let mut tx = doc.transaction();
            reconcile(&mut tx, &LspAgent::default()).unwrap();
            tx.commit();
        });

        let mut mock_client = MockRetryingClient::new();
        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_mock = call_count.clone();
        mock_client.expect_inference().times(2).returning(move |request, _| {
            let call_index = call_count_for_mock.fetch_add(1, Ordering::SeqCst);
            if call_index == 0 {
                Ok(r#"{"action":"launch_app","app":"RoundedView{\n    width: Fill height: Fit\n    on_render: ||{ if (1 == 1) { ui.title.set_text(\"bad\") } }\n}"}"#.to_string())
            } else {
                assert!(request.contains("could not be launched"));
                Ok(r#"{"action":"launch_app","app":"RoundedView{\n    width: Fill height: Fit\n    new_batch: true\n    draw_bg.color: #x1e1e2e\n    draw_bg.border_radius: 10.0\n    title := Label{text: \"Calendar\" draw_text.color: #fff}\n}"}"#.to_string())
            }
        });
        mock_client.expect_notify_shutdown().times(0);

        let client: Arc<dyn InferenceClient> = Arc::new(mock_client);
        let app = RecordingApp {
            launched: tokio::sync::Mutex::new(vec![]),
        };

        let (tx, rx) = oneshot::channel();
        handle_chat_request(
            ChatRequest {
                content: "calendar app".to_string(),
                model: Some("gpt-5.4".to_string()),
                responder: tx,
            },
            AppRuntime::Makepad,
            &doc_handle,
            &client,
            &app,
        )
        .await;

        assert_eq!(rx.await.unwrap(), None);

        let launched = app.launched.lock().await;
        assert_eq!(launched.len(), 1);
        assert!(launched[0].1.contains("title := Label{text: \"Calendar\""));
        assert_eq!(call_count.load(Ordering::SeqCst), 2);
    }
}
