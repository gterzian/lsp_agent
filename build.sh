#!/bin/bash
set -e

ROOT_DIR=$(cd "$(dirname "$0")" && pwd)
cd "$ROOT_DIR"

echo "=== Building Rust Server ==="
cd vs_code_lsp/server
cargo fmt
cargo build
cd ../..

echo "=== Building Rust Web Client ==="
cd web
cargo fmt
cargo build
cd ..

echo "=== Building TypeScript Client ==="
cd vs_code_lsp/client
# Only run install if node_modules doesn't exist to save time
if [ ! -d "node_modules" ]; then
    npm install
fi
npm run compile
cd ..

echo "=== Build Complete ==="
