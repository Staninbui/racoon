# Build stage
FROM rust:1.92-bookworm AS builder

WORKDIR /app

# Copy everything needed for build
COPY Cargo.toml Cargo.lock config_template.toml ./
COPY src ./src

# Build the application
RUN cargo build --release

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