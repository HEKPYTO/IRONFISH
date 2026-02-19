FROM rust:1-bookworm AS builder
WORKDIR /app
RUN apt-get update && apt-get install -y protobuf-compiler git build-essential && rm -rf /var/lib/apt/lists/*

RUN git clone https://github.com/official-stockfish/Stockfish.git && \
    cd Stockfish/src && \
    ARCH=$(uname -m) && \
    if [ "$ARCH" = "aarch64" ]; then \
        make -j $(nproc) build ARCH=armv8; \
    else \
        make -j $(nproc) build ARCH=x86-64; \
    fi && \
    mv stockfish /app/stockfish_bin

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
RUN cargo build --release --bin ironfish-server --bin ironfish

FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y \
    ca-certificates \
    libstdc++6 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/stockfish_bin /usr/local/bin/stockfish
COPY --from=builder /app/target/release/ironfish-server /usr/local/bin/
COPY --from=builder /app/target/release/ironfish /usr/local/bin/

RUN mkdir -p /var/lib/ironfish /etc/ironfish

EXPOSE 8080 8081

ENV RUST_LOG=info
ENV IRONFISH_CONFIG=/etc/ironfish/config.toml
ENV STOCKFISH_PATH=/usr/local/bin/stockfish

ENTRYPOINT ["ironfish-server"]