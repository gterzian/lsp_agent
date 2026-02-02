# Build Instructions

**Note: Always build the project after having made a change.**

To build the entire project (Rust server and TypeScript client), run the build script in the root directory:

```bash
./build.sh
```

## Individual Components

### Server (Rust)
The server is located in the `server/` directory.
To build it manually:
```bash
cd server
cargo build
```

### Client (VS Code Extension)
The client is located in the `client/` directory.
To build it manually:
```bash
cd client
npm install
npm run compile
```

### Dependency Management

If you need to install updated dependencies for the web project:

```bash
chmod +x install_deps.sh
./install_deps.sh
```
