# Development

## Setup

1.  **Install Rust:** `rustup update`
2.  **Install Stockfish:** Ensure `stockfish` is in your PATH.
3.  **Install Protoc:** Required for gRPC code generation.

## Testing

*   **Unit Tests:** `cargo test`
*   **Docker Tests:** `cargo test --package ironfish-tests` (Requires Docker running)

## Code Quality

We use a pre-commit hook to enforce standards.
*   **Clippy:** `cargo clippy --workspace --all-targets -- -D warnings`
*   **Fmt:** `cargo fmt --all --check`

## Project Structure

*   `crates/ironfish-api`: Web server (Axum/Tonic).
*   `crates/ironfish-core`: Shared types and logic.
*   `crates/ironfish-cluster`: Networking, Consensus, Gossip.
*   `crates/ironfish-stockfish`: Process management.
*   `crates/ironfish-server`: Main entrypoint.
