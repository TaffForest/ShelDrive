# ShelDrive

A desktop application that mounts the [Shelby Protocol](https://shelby.xyz) decentralised hot storage network as a native drive on your system.

- **macOS**: `/Volumes/ShelDrive`
- **Linux**: `/mnt/sheldrive`
- **Windows**: `S:\` *(planned)*

Copy files into the drive and they're pinned to the Shelby network. Read files back and they're retrieved on demand. Delete files and they're unpinned.

## Architecture

```
[OS filesystem calls]
        |
    [FUSE / WinFSP]
        |
    [ShelDriveFS]  ── SQLite CID index (~/.sheldrive/index.db)
        |                   |
    [ShelbyBridge] ── [Disk cache (~/.sheldrive/cache/)]
        |
    [Node.js sidecar]  (JSON-RPC over stdio)
        |
    [@shelby-protocol/sdk]
        |
    [Shelby Network]
```

## Tech Stack

| Layer | Technology |
|-------|-----------|
| App shell | Tauri v2 (Rust + WebView) |
| Filesystem | FUSE via `fuser` crate (macOS/Linux) |
| Storage SDK | `@shelby-protocol/sdk/node` |
| Local index | SQLite via `rusqlite` |
| Frontend | React + TypeScript |
| IPC | JSON-RPC 2.0 over stdin/stdout |

## Prerequisites

### macOS

```bash
# Install FUSE-T (userspace FUSE — no kernel extension)
brew install --cask fuse-t

# Or install macFUSE (requires reduced security on Apple Silicon)
brew install --cask macfuse
```

### Linux

```bash
# Debian/Ubuntu
sudo apt install libfuse3-dev fuse3

# Fedora
sudo dnf install fuse3-devel fuse3
```

### All platforms

- [Rust](https://rustup.rs) 1.85+
- [Node.js](https://nodejs.org) 22+

## Setup

```bash
# Clone
git clone <repo-url> && cd sheldrive

# Install frontend deps
npm install

# Build sidecar
cd sidecar && npm install && npm run build && cd ..

# Configure Shelby credentials
mkdir -p ~/.sheldrive
cat > ~/.sheldrive/config.toml << 'EOF'
[shelby]
network = "SHELBYNET"
api_key = "<your-api-key>"
rpc_url = "https://api.shelbynet.shelby.xyz/shelby"
private_key = "<your-ed25519-private-key-hex>"
EOF
```

## Development

```bash
# Run in dev mode (hot-reload frontend + Rust rebuild)
npx tauri dev
```

## Build

```bash
# Production build — generates platform installer
npx tauri build
```

Build outputs:
- macOS: `src-tauri/target/release/bundle/dmg/ShelDrive_*.dmg`
- Linux: `src-tauri/target/release/bundle/deb/sheldrive_*.deb`
- Linux: `src-tauri/target/release/bundle/appimage/ShelDrive_*.AppImage`

## Configuration

Config file: `~/.sheldrive/config.toml`

| Key | Description | Default |
|-----|-------------|---------|
| `network` | Shelby network | `SHELBYNET` |
| `api_key` | Shelby API key | *(none)* |
| `rpc_url` | Shelby RPC endpoint | *(auto)* |
| `private_key` | Ed25519 private key (hex) | *(none — mock mode)* |

Without a private key, ShelDrive runs in **mock mode** — files are stored in-memory only and not pinned to the network.

## Data locations

| Path | Contents |
|------|----------|
| `~/.sheldrive/config.toml` | Configuration |
| `~/.sheldrive/index.db` | SQLite path→CID mapping |
| `~/.sheldrive/cache/` | LRU file content cache (512 MB default) |

## Project Structure

```
sheldrive/
├── src-tauri/src/
│   ├── main.rs          # Entry point
│   ├── lib.rs           # Tauri setup, tray, module wiring
│   ├── commands.rs      # IPC commands (mount, unmount, status)
│   ├── state.rs         # App state (mount status, FUSE thread)
│   ├── cache.rs         # LRU disk cache
│   ├── db/
│   │   ├── schema.rs    # SQLite migrations
│   │   └── index.rs     # CID index CRUD
│   ├── fs/
│   │   └── fuse_driver.rs  # FUSE filesystem implementation
│   └── bridge/
│       └── shelby.rs    # Sidecar process manager + JSON-RPC client
├── src/                 # React frontend (tray panel UI)
├── sidecar/             # Node.js Shelby SDK bridge
│   └── src/
│       ├── index.ts     # JSON-RPC server (stdio)
│       ├── shelby-client.ts  # SDK wrapper
│       └── protocol.ts  # RPC type definitions
└── .github/workflows/
    └── build.yml        # CI: build all platforms + release
```

## License

MIT
