# API Reference

## Authentication

*   **Admin Actions:** Require `X-Admin-Key` header. Configured via `IRONFISH_ADMIN_KEY` env var.
*   **User Actions:** Require `Authorization: Bearer <TOKEN>` header.

## REST API

### Health
`GET /v1/health`
Returns 200 OK if the node is running.

### Metrics
`GET /v1/metrics`
**Auth:** Bearer
Returns system metrics:
```json
{
  "cpu_usage": 12.5,
  "memory_usage": 0.45,
  "active_analyses": 2
}
```

### Analyze
`POST /v1/analyze`
**Auth:** Bearer
**Body:**
```json
{
  "fen": "...",
  "depth": 20
}
```

## GraphQL API
Endpoint: `/graphql`

### Query: Cluster Status
```graphql
query {
  clusterStatus {
    nodes {
      id
      state
      address
    }
    leader
    healthy
  }
}
```

## gRPC API
Service: `ChessAnalysis`
*   `Analyze(AnalyzeRequest) returns (AnalyzeResponse)`
