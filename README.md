# Ironfish

Ironfish is a **fault-tolerant, distributed chess analysis engine** built with Rust. It leverages **Stockfish**, **Docker**, and **Gossip protocols** to create a resilient, scalable, and self-healing cluster for chess position analysis.

## Key Features

- **Distributed Analysis:** Distributes chess analysis workload across a cluster of nodes.
- **Fault Tolerance:** Built-in leader election (Hybrid Raft/Bully) and self-healing capabilities. If a node dies, the cluster recovers automatically.
- **Auto-Discovery:** Nodes discover each other via Static config, UDP Multicast, or DNS (Cloud/Kubernetes).
- **Load Balancing:** CPU-aware and Queue-aware load balancing strategies.
- **Secure API:** Token-based authentication (`X-Admin-Key` for management, Bearer tokens for usage).
- **Multi-Protocol Support:** Single port (8080) exposes REST, gRPC, and GraphQL APIs.
- **Observability:** Built-in metrics (`/v1/metrics`) tracking CPU, Memory, and Engine usage.

## Architecture

Ironfish operates as a cluster of identical nodes. Each node runs:
1.  **API Layer:** Axum (REST/GraphQL) + Tonic (gRPC) multiplexed on port 8080.
2.  **Cluster Service:** Handles membership, gossip, and failure detection.
3.  **Stockfish Pool:** Manages a pool of Stockfish engine processes for analysis.

## Quick Start

### Prerequisites
*   Docker & Docker Compose
*   (Optional) Rust toolchain for local development

### Running a Cluster

1.  **Start the cluster:**
    ```bash
    docker compose up -d
    ```
    This spins up 3 nodes (`node1`, `node2`, `node3`).

2.  **Create an Admin Token:**
    ```bash
    # Use the default admin key "cluster-admin-secret" defined in docker-compose.yml
    curl -X POST http://localhost:8080/_admin/tokens \
      -H "X-Admin-Key: cluster-admin-secret" \
      -H "Content-Type: application/json" \
      -d '{"name": "my-token"}'
    ```
    *Response:* `{"token": "iff_..."}`

3.  **Analyze a Position:**
    ```bash
    curl -X POST http://localhost:8080/v1/analyze \
      -H "Authorization: Bearer <YOUR_TOKEN>" \
      -H "Content-Type: application/json" \
      -d '{ 
        "fen": "rnbqkbnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQKBNR w KQkq - 0 1",
        "depth": 15
      }'
    ```

### API Endpoints

| Method | Path | Description | Auth |
| :--- | :--- | :--- | :--- |
| `GET` | `/v1/health` | Node health status | None |
| `GET` | `/v1/metrics` | System & Cluster metrics | Bearer |
| `POST` | `/v1/analyze` | Analyze FEN position | Bearer |
| `POST` | `/graphql` | GraphQL Endpoint | Bearer |
| `POST` | `/_admin/tokens` | Create API Token | Admin Key |
| `DELETE` | `/_admin/tokens/:id` | Revoke API Token | Admin Key |

## Development

### Pre-commit Hooks
The project includes a pre-commit hook to ensure code quality.
```bash
cp pre-commit .git/hooks/pre-commit
chmod +x .git/hooks/pre-commit
```

### Running Tests
```bash
cargo test --workspace
```
Note: Docker integration tests run sequentially to avoid port conflicts.

## License
MIT