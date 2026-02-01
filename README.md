# Ironfish

<div align="left">

[![Status](https://img.shields.io/badge/Status-Active-success?style=flat-square)]()
[![Rust](https://img.shields.io/badge/Rust-1.75%2B-orange?style=flat-square&logo=rust)](https://www.rust-lang.org)
[![License](https://img.shields.io/badge/License-MIT-blue?style=flat-square)](LICENSE)
[![Docker](https://img.shields.io/badge/Docker-Supported-2496ED?style=flat-square&logo=docker)]()
[![Protocol](https://img.shields.io/badge/Protocol-gRPC%20%7C%20REST%20%7C%20GraphQL-purple?style=flat-square)]()

</div>

<div align="center">
  <img src="assets/logo.png" alt="Ironfish Logo" width="256" />
</div>

A distributed, fault-tolerant chess position analysis system using Stockfish, built entirely in Rust.

## Features

- **Multi-Protocol API** - REST, gRPC, and GraphQL on separate ports
- **HA Clustering** - Automatic node discovery and gossip-based token replication
- **Token Authentication** - Secure API access with cluster-wide token synchronization
- **Stockfish Integration** - Engine pool for concurrent position analysis
- **Admin CLI** - Cluster management and token administration

## Quick Start

### Single Node

```bash
# Build
cargo build --release

# Run (requires Stockfish installed)
STOCKFISH_PATH=/usr/local/bin/stockfish ./target/release/ironfish-server
```

### Docker Cluster (Example 3 nodes)

```bash
# Start cluster
docker compose up -d

# Check health
curl http://localhost:8080/v1/health
curl http://localhost:8082/v1/health
curl http://localhost:8084/v1/health

# Create API token
curl -X POST -H "X-Admin-Key: cluster-admin-secret" \
  -H "Content-Type: application/json" -d '{}' \
  http://localhost:8080/_admin/tokens

# Analyze position
curl -X POST -H "Authorization: Bearer <token>" \
  -H "Content-Type: application/json" \
  -d '{"fen": "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", "depth": 15}' \
  http://localhost:8080/v1/analyze
```

## API Endpoints

### REST API (Port 8080)

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/v1/analyze` | Analyze chess position |
| POST | `/v1/bestmove` | Get best move only |
| GET | `/v1/health` | Health check (public) |
| GET | `/v1/metrics` | Engine metrics |

### Admin API (Requires X-Admin-Key header)

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/_admin/tokens` | List all tokens |
| POST | `/_admin/tokens` | Create new token |
| DELETE | `/_admin/tokens/:id` | Revoke token |
| GET | `/_admin/cluster/status` | Cluster membership |

### gRPC API (Port 8081)

```bash
# List services
grpcurl -plaintext localhost:8081 list

# Analyze position
grpcurl -plaintext -H "authorization: Bearer <token>" \
  -d '{"fen": "startpos", "depth": 10}' \
  localhost:8081 chess.ChessAnalysis/Analyze
```

### GraphQL API (Port 8080/graphql)

```graphql
query {
  analyze(fen: "rnbqkbnr/pppppppp/8/8/4P3/8/PPPP1PPP/RNBQKBNR b KQkq - 0 1", depth: 15) {
    bestMove { from to }
    evaluation { scoreType value }
  }
}
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `IRONFISH_NODE_ID` | auto | Node identifier |
| `IRONFISH_BIND_ADDRESS` | 0.0.0.0:8080 | Listen address |
| `IRONFISH_CLUSTER_PEERS` | - | Comma-separated peer list |
| `IRONFISH_TOKEN_SECRET` | dev-secret | Shared HMAC secret for tokens |
| `IRONFISH_ADMIN_KEY` | - | Admin API authentication key |
| `STOCKFISH_PATH` | /usr/local/bin/stockfish | Path to Stockfish binary |

### Config File (config/default.toml)

```toml
[node]
id = "auto"
bind_address = "0.0.0.0:8080"
data_dir = "/var/lib/ironfish"

[stockfish]
binary_path = "/usr/bin/stockfish"
pool_size = 4
default_depth = 20

[cluster]
enabled = true
heartbeat_interval_ms = 1000
gossip_interval_ms = 5000

[discovery]
static_peers = ["node2:8080", "node3:8080"]
multicast_enabled = true

[auth]
enabled = true
token_ttl_days = 365
```

## Project Structure

```
ironfish/
├── Cargo.toml                 # Workspace root
├── Dockerfile                 # Multi-stage build
├── docker-compose.yml         # 3-node cluster setup
├── crates/
│   ├── ironfish-core/         # Core types, traits, errors
│   ├── ironfish-stockfish/    # Stockfish engine pool
│   ├── ironfish-auth/         # Token management & middleware
│   ├── ironfish-cluster/      # Clustering & gossip
│   │   ├── src/
│   │   │   ├── discovery/     # Peer discovery (static, multicast)
│   │   │   ├── consensus/     # Raft + Bully algorithms
│   │   │   ├── gossip.rs      # Token replication
│   │   │   └── network.rs     # TCP transport
│   ├── ironfish-api/          # REST, gRPC, GraphQL
│   │   ├── proto/             # Protobuf definitions
│   │   └── src/
│   │       ├── rest/          # Axum handlers
│   │       ├── grpc/          # Tonic service
│   │       └── graphql/       # async-graphql schema
│   ├── ironfish-cli/          # Admin CLI tool
│   └── ironfish-server/       # Main binary
└── config/
    └── default.toml           # Default configuration
```

## Development

```bash
# Run tests
cargo test --all

# Run with logging
RUST_LOG=info cargo run -p ironfish-server

# Format code
cargo fmt --all

# Lint
cargo clippy --all -- -D warnings
```

## Cluster Features

### Token Replication

Tokens created on any node are automatically replicated to all cluster nodes via gossip protocol. Token revocations are also propagated cluster-wide.

### Auto-Discovery

Nodes discover each other using:
1. **Static peers** - Hostname:port list (supports DNS resolution)
2. **Multicast** - UDP discovery on local network (239.255.42.98:7878)

### Shared Authentication

All nodes must use the same `IRONFISH_TOKEN_SECRET` to validate tokens created by other nodes.

## License

MIT
