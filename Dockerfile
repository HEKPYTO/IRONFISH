FROM rust:latest AS builder

WORKDIR /app

RUN apt-get update && apt-get install -y \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

COPY Cargo.toml Cargo.lock ./
COPY crates ./crates

RUN cargo build --release --bin ironfish-server --bin ironfish

FROM debian:bookworm-slim AS stockfish-builder

RUN apt-get update && apt-get install -y \
    git \
    make \
    g++ \
    curl \
    && rm -rf /var/lib/apt/lists/*

ARG STOCKFISH_VERSION=sf_18
ARG TARGETARCH

WORKDIR /build
RUN git clone --depth 1 --branch ${STOCKFISH_VERSION} https://github.com/official-stockfish/Stockfish.git && \
    cd Stockfish/src && \
    if [ "${TARGETARCH}" = "arm64" ]; then \
        make -j$(nproc) build ARCH=armv8; \
    else \
        make -j$(nproc) build ARCH=x86-64; \
    fi && \
    find . -type f -executable -name "stockfish*" -exec mv {} /usr/local/bin/stockfish \;

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libstdc++6 \
    curl \
    && rm -rf /var/lib/apt/lists/*

COPY --from=stockfish-builder /usr/local/bin/stockfish /usr/local/bin/stockfish
COPY --from=builder /app/target/release/ironfish-server /usr/local/bin/
COPY --from=builder /app/target/release/ironfish /usr/local/bin/

RUN mkdir -p /var/lib/ironfish /etc/ironfish

EXPOSE 8080 8081

ENV RUST_LOG=info
ENV IRONFISH_CONFIG=/etc/ironfish/config.toml
ENV STOCKFISH_PATH=/usr/local/bin/stockfish

ENTRYPOINT ["ironfish-server"]
