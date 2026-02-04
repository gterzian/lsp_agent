# LSP Web Agent

This project is a pre-alpha VS Code extension that runs an agent via an LSP server. Inference goes via GitHub Copilot. The agent can write and launch apps in a webview.

Interaction with the agent goes through the `@web-agent` chat participant.

The agent can answer questions about the code it writes and iterate on it; each iteration launches a new webview. It can see a list of documents URIs, but cannot read their contents directly.

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
4. Open the Chat view and select the “web-agent” participant.
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

## Maybe Useful Test Cases

- “summarize active doc” with an (untitled) document open.
- app to fetch and summarize web pages (prevents prompt injection to the main agent).
- “list open documents” to verify document tracking.
- “create a simple HTML todo app” to validate web app launch.
- Basic AI chat web app using inference through the extension.
- "I want to play tic-tac-toe with an opponent using AI inference."

<img width="1124" height="619" alt="Screenshot 2026-02-05 at 12 19 21 AM" src="https://github.com/user-attachments/assets/6c59e5de-c142-402c-8af3-2f4d5bee9b90" />


<img width="1145" height="793" alt="Screenshot 2026-02-04 at 11 19 53 PM" src="https://github.com/user-attachments/assets/14de254f-ef53-4d89-8bc3-d3847e42b3eb" />

