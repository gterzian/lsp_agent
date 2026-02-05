# LSP Web Agent

An agent that can write and launch web apps from your workspace.

This project is a pre-alpha VS Code extension that runs an agent via an LSP server. Inference goes via GitHub Copilot. The agent can write and launch apps in a webview.

Interaction with the agent goes through the `@web-agent` chat participant.

The agent can answer questions about running apps and the code it writes and iterate on those (for now each iteration launches a new webview).

The agent can also see a list of open document URIs, but cannot read their contents directly. It also doesn't have direct access to the internet. In order to process either local or remote content, it therefore must write a web app and make sub-inference calls. Isolating the main agent from actual content limits prompt injection risk.

The app runs in a standard system webview through [wry](https://docs.rs/wry/latest/wry/), without additional sandboxing.
The app has access to workspace documents and inference by way of custom protocols.

The main use case is having the agent write an app that does sub inference on data with prompt injection potential.

## Requirements

- VS Code with an active Github Copilot extension.

## Quick Start

1. Download the repo and open it in VS Code.
2. Build everything:
   - `./build.sh`
3. Run the extension in debug mode:
   - In VS Code, open Run and select “Start debugging”.
4. Open the Chat view in the debugging window and prompt `@web-agent`.
5. The model used, both as the main agent and for app inference, is the one you select in the chat.


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
- **LSP server (Rust)** hosts the agent core, owns the inference client, and writes requests/responses into the shared document.
- **Web client (Rust + wry)** runs in a separate process, renders HTML apps, and uses custom `wry://` protocols to request inference or document reads. It never calls inference directly; it writes requests into the shared document and listens for responses.

Data flow is intentionally split across the process boundary to prevent the webview from directly invoking inference or accessing documents without going through the agent’s request/response flow.

## Maybe Useful Test Cases

- “summarize active doc” with an (untitled) document open.
- app to fetch and summarize web pages (prevents prompt injection to the main agent).
- “list open documents” to verify document tracking.
- “create a simple HTML todo app” to validate web app launch.
- Basic AI chat web app using inference through the extension.
- "I want to play tic-tac-toe with an opponent using AI inference."

<img width="1124" height="619" alt="Screenshot 2026-02-05 at 12 19 21 AM" src="https://github.com/user-attachments/assets/6c59e5de-c142-402c-8af3-2f4d5bee9b90" />


<img width="1145" height="793" alt="Screenshot 2026-02-04 at 11 19 53 PM" src="https://github.com/user-attachments/assets/14de254f-ef53-4d89-8bc3-d3847e42b3eb" />

## Ideas for Roadmap

- Persist apps like bookmarks
- Clearer data boundaries (when a local doc is used in an app, prevent extraction over the internet?)
- Additional sandboxing for the web app?