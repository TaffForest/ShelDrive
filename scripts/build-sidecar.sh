#!/bin/bash
# Build the sidecar bundle for production
set -e

SIDECAR_DIR="$(cd "$(dirname "$0")/../sidecar" && pwd)"
OUT_DIR="$(dirname "$0")/../src-tauri/sidecar-dist"
mkdir -p "$OUT_DIR"
OUT_DIR="$(cd "$OUT_DIR" && pwd)"

echo "Building sidecar..."
cd "$SIDECAR_DIR"

# Compile TypeScript
npm run build

# Bundle with esbuild (externalize SDK — it needs node_modules at runtime)
npx esbuild dist/index.js \
  --bundle \
  --platform=node \
  --target=node20 \
  --format=esm \
  --outfile="$OUT_DIR/sidecar.mjs" \
  --external:@shelby-protocol/sdk \
  --external:@aptos-labs/ts-sdk \
  --external:@shelby-protocol/clay-codes

# Copy ALL node_modules — ensures transitive deps are included
rm -rf "$OUT_DIR/node_modules"
cp -r node_modules "$OUT_DIR/node_modules"

echo "Sidecar built → $OUT_DIR"
ls -lh "$OUT_DIR/sidecar.mjs"
du -sh "$OUT_DIR/node_modules" | awk '{print "node_modules: " $1}'
