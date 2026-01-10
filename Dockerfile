# Build stage
FROM rust:1.85-bookworm AS builder

WORKDIR /app

# Copy manifests and config template
COPY Cargo.toml Cargo.lock config_template.toml ./

# Create dummy src for caching dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs && mkdir -p src/ai src/gcp src/storage src/ui && touch src/lib.rs src/ai/mod.rs src/gcp/mod.rs src/storage/mod.rs src/ui/mod.rs

# Build dependencies (this layer will be cached)
RUN cargo build --release && rm -rf src

# Copy actual source code
COPY src ./src

# Build the application
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Install dependencies
RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Copy binary from builder
COPY --from=builder /app/target/release/gcp-racoon /usr/local/bin/gcp-racoon

# Create non-root user
RUN useradd -m -s /bin/bash racoon
USER racoon
WORKDIR /home/racoon

# Set default command
ENTRYPOINT ["gcp-racoon"]
CMD ["--help"]