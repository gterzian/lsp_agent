#!/bin/bash
set -e

ROOT_DIR=$(cd "$(dirname "$0")" && pwd)
cd "$ROOT_DIR"

echo "=== Testing and Building Traits ==="
cd traits
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo build
cd ..

echo "=== Testing and Building Shared Document ==="
cd agent/shared_document
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo build
cd ../..

echo "=== Testing and Building Prompts ==="
cd agent/prompts
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo build
cd ../..

echo "=== Testing and Building Agent Core ==="
cd agent
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo build
cd ..

echo "=== Testing and Building Rust Server ==="
cd vs_code_lsp/server
cargo fmt
cargo clippy -- -D warnings
cargo test
cargo build
cd ../..

echo "=== Testing and Building Rust Web Client ==="
cd web
cargo fmt
cargo clippy -- -D warnings
cargo test
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
