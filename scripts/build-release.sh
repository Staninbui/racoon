#!/bin/bash
set -e

VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
DIST_DIR="dist/v${VERSION}"

echo "🦝 Building racoon v${VERSION}"
echo "================================"

mkdir -p "$DIST_DIR"

build_target() {
    local target=$1
    local ext=$2
    local archive_ext=$3
    local use_cross=${4:-false}

    echo "📦 Building for $target..."

    if [ "$use_cross" = "true" ]; then
        if ! command -v cross &> /dev/null; then
            echo "⚠️  cross not installed, skipping $target"
            echo "   Install with: cargo install cross --git https://github.com/cross-rs/cross"
            return 1
        fi
        if ! docker info &> /dev/null; then
            echo "⚠️  Docker not running, skipping $target"
            return 1
        fi
        cross build --release --target "$target"
    else
        cargo build --release --target "$target"
    fi

    local binary="target/${target}/release/racoon${ext}"
    local archive_name="racoon-${target}${archive_ext}"

    if [ -f "$binary" ]; then
        if [ "$archive_ext" = ".zip" ]; then
            zip -j "${DIST_DIR}/${archive_name}" "$binary"
        else
            tar -czvf "${DIST_DIR}/${archive_name}" -C "target/${target}/release" "racoon${ext}"
        fi
        echo "✅ Created ${DIST_DIR}/${archive_name}"
    else
        echo "❌ Failed to build for $target"
        return 1
    fi
}

# Detect OS and build appropriate targets
if [[ "$OSTYPE" == "darwin"* ]]; then
    echo "🍎 Building macOS targets..."
    
    # Add targets if not already added
    rustup target add aarch64-apple-darwin 2>/dev/null || true
    rustup target add x86_64-apple-darwin 2>/dev/null || true
    
    build_target "aarch64-apple-darwin" "" ".tar.gz" false
    build_target "x86_64-apple-darwin" "" ".tar.gz" false
    
    # Linux targets require cross (Docker)
    echo ""
    echo "🐧 Building Linux targets (requires Docker + cross)..."
    build_target "x86_64-unknown-linux-gnu" "" ".tar.gz" true || true
    build_target "aarch64-unknown-linux-gnu" "" ".tar.gz" true || true

elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
    echo "🐧 Building Linux targets..."
    build_target "x86_64-unknown-linux-gnu" "" ".tar.gz" false
    build_target "aarch64-unknown-linux-gnu" "" ".tar.gz" true || true

elif [[ "$OSTYPE" == "msys"* ]] || [[ "$OSTYPE" == "cygwin"* ]]; then
    echo "🪟 Building Windows target..."
    build_target "x86_64-pc-windows-msvc" ".exe" ".zip" false
fi

echo ""
echo "🎉 Release builds complete!"
echo "📁 Distribution files in: $DIST_DIR"
ls -la "$DIST_DIR" 2>/dev/null || echo "(no files yet)"

echo ""
echo "💡 Tip: For full cross-platform builds, use GitHub Actions (push a tag)"
echo "   git tag v${VERSION} && git push origin v${VERSION}"