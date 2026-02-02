# Deployment

## Docker Compose (Recommended)

The easiest way to run a cluster.

```yaml
version: '3.8'
services:
  node1:
    image: ironfish:latest
    environment:
      - IRONFISH_ADMIN_KEY=secret
      - IRONFISH_CLUSTER_PEERS=node2:8080,node3:8080
```

## Environment Variables

| Variable | Description | Default |
| :--- | :--- | :--- |
| `RUST_LOG` | Logging level (info, debug, trace) | `info` |
| `IRONFISH_BIND_ADDRESS` | Address to bind to | `0.0.0.0:8080` |
| `IRONFISH_ADMIN_KEY` | Secret key for admin operations | `cluster-admin-secret` |
| `IRONFISH_TOKEN_SECRET` | Secret for signing JWTs | **MUST CHANGE IN PROD** |
| `IRONFISH_CLUSTER_PEERS` | Comma-separated list of peers | `""` |
| `STOCKFISH_PATH` | Path to Stockfish binary | `/usr/local/bin/stockfish` |

## Kubernetes

Deploy as a `StatefulSet` with a Headless Service for DNS discovery.
Enable `DnsDiscovery` mode in configuration.
