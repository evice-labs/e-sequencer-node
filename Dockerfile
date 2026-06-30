# Stage 1: Build
FROM rust:1.90-bookworm AS builder

WORKDIR /build

# Copy library dependency first (for better layer caching)
COPY e-sequencer/ ./e-sequencer/
COPY e-sequencer-node/ ./e-sequencer-node/

WORKDIR /build/e-sequencer-node
RUN cargo build --release

# Stage 2: Runtime
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/e-sequencer-node/target/release/e-sequencer-node /usr/local/bin/

ENTRYPOINT ["e-sequencer-node"]
