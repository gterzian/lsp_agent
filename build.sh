#!/bin/bash
set -e

echo "=== Building Rust Server ==="
cd server
cargo build
cd ..

echo "=== Building TypeScript Client ==="
cd client
# Only run install if node_modules doesn't exist to save time
if [ ! -d "node_modules" ]; then
    npm install
fi
npm run compile
cd ..

echo "=== Build Complete ==="
