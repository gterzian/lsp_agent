# Build Instructions

**Note: Always build the project after having made a change.**

To build the entire project (Rust server and TypeScript client), run the build script in the root directory:

```bash
./build.sh
```

## Individual Components

### Server (Rust)
The server is located in the `vs_code_lsp/server/` directory and is a thin LSP host that delegates to the agent crate.
To build it manually:
```bash
cd vs_code_lsp/server
cargo build
```

### Agent Core (Rust)
The agent core (automerge infra, shared document types, and prompt building) lives in the `agent/` crate.
It is built as a dependency of the server and web crates, but you can build it directly:
```bash
cd agent
cargo build
```

### Traits (Rust)
Public interfaces shared across crates (for example `Agent` and `AgentClient`) live in the `traits/` crate.
It is built as a dependency of the server and agent crates, but you can build it directly:
```bash
cd traits
cargo build
```

### Client (VS Code Extension)
The client is located in the `vs_code_lsp/client/` directory.
To build it manually:
```bash
cd vs_code_lsp/client
npm install
npm run compile
```

### Dependency Management

If you need to install updated dependencies for the web project:

```bash
chmod +x install_deps.sh
./install_deps.sh
```
