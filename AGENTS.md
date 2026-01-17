# Build Instructions

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
