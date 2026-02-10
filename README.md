# LSP Web Agent

An agent that can write and launch web apps from your workspace.

This project is a pre-alpha VS Code extension that runs an agent via an LSP server. Inference goes via GitHub Copilot. The agent can write and launch apps in a webview.

Interaction with the agent goes through the `@web-agent` chat participant.

The agent can answer questions about running apps and the code it writes and iterate on those (for now each iteration launches a new webview).

The agent can also see a list of open document URIs, but cannot read their contents directly. It also doesn't have direct access to the internet. In order to process either local or remote content, it therefore must write a web app and make sub-inference calls. Isolating the main agent from actual content limits prompt injection risk.

The agent can also see a list of values stored by apps. Apps can store and retrieve values from a shared key-value store, and get notified when those change, enabling persistence and data sharing between apps.

The app runs in a standard system webview through [wry](https://docs.rs/wry/latest/wry/), without additional sandboxing.
The app has access to workspace documents, inference, and the shared key-value store by way of custom protocols.

The main use case is having the agent write an app that does sub inference on data with prompt injection potential.

## Requirements

- VS Code with an active Github Copilot extension.

## Quick Start

1. Git clone the repo and open it in VS Code.
2. Build everything:
   - `./build.sh`
3. Run the extension in debug mode:
   - In VS Code, open Run and select “Start debugging”.
4. Open the Chat view in the debugging window and prompt `@web-agent`.
5. The model used, both as the main agent and for app inference, is the one you select in the chat(auto defaults to gpt-5-mini).
6. See [below](#maybe-useful-test-cases) for prompt ideas.


## Repository Structure

- `vs_code_lsp`
   - `client`: VS Code extension (TypeScript)
   - `server`: Thin LSP server (Rust)
- `agent`: Core agent logic and Automerge-backed data model
   - `prompts`: Prompt templates and builders
   - `shared_document`: Shared document types (Automerge schema)
- `traits`: Shared public interfaces
- `web`: Web client that renders HTML apps and handles custom protocols

## Process Architecture

This project runs as multiple processes with a shared Automerge document as the coordination layer.

- **VS Code extension (TypeScript)** spawns the Rust LSP server and forwards editor events.
- **LSP server (Rust)** hosts the agent core, owns the inference client, and manages the shared document (including requests/responses and stored values).
- **Web client (Rust + wry)** runs in a separate process, renders HTML apps, and uses custom `wry://` protocols to request inference, read documents, or access stored values. It never calls inference directly; it writes requests into the shared document and listens for responses.

Data flow is intentionally split across the process boundary to prevent the webview from directly invoking inference or accessing documents without going through the agent’s request/response flow.

This modular split also makes it possible to swap in other editor front-ends or alternative web runtimes. Note that using a crdt for communication is an implementation detail and not part of the [interface](https://github.com/gterzian/lsp_agent/blob/main/traits/src/lib.rs).

### Design Principles

The architecture prioritizes **security** and **state synchronization**:

- **Strong Sandboxing**: By splitting the system into a privileged Server and an unprivileged Web Client, we create a hard security boundary. The Web Client (running AI-generated apps) cannot call inference or read files directly. It must request these actions via the shared document, allowing the Server to act as a gatekeeper against prompt injection.
- **CRDTs as the Communication Bus**: Using **Automerge** creates a "shared brain" where the state (open docs, chat history, app state) is unified. This decouples the processes—the Web process simply updates the state to request inference, and the Server updates it to provide the response. This also simplifies persistence.
- **Thin LSP Server**: The server mostly translates VS Code events into shared document updates, keeping protocol logic clean and separating editor integration from agent intelligence.

## Maybe Useful Test Cases

- “summarize active doc” with an (untitled) document open.
- app to fetch and summarize web pages (prevents prompt injection to the main agent).
- “list open documents” to verify document tracking.
- “create a simple HTML todo app” to validate web app launch.
- Basic AI chat web app using inference through the extension.
- "I want to play tic-tac-toe with an opponent using AI inference."
- Tic-tac-toe game storing scoreboard; another app showing live schore.
- "Create a note taking app that persists notes."
- "list stored values" to see what data has been persisted by apps.

<img width="1124" height="619" alt="Screenshot 2026-02-05 at 12 19 21 AM" src="https://github.com/user-attachments/assets/6c59e5de-c142-402c-8af3-2f4d5bee9b90" />


<img width="1145" height="793" alt="Screenshot 2026-02-04 at 11 19 53 PM" src="https://github.com/user-attachments/assets/14de254f-ef53-4d89-8bc3-d3847e42b3eb" />

## Ideas for Roadmap

- Persist apps like bookmarks
- Clearer data boundaries (when a local doc is used in an app, prevent extraction over the internet?)
- Additional sandboxing for the web app.
- Manage apps through a markdown document in the workspace(ai writes state to doc, if user removes app from list: ai closes or deletes app): browser chrome as markdown doc.
- Endpoint for app to write state to automerge doc.