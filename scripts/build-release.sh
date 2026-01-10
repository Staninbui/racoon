#!/bin/bash
set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

VERSION=$(grep '^version' Cargo.toml | head -1 | cut -d'"' -f2)
BINARY_NAME="gcp-racoon"

usage() {
    echo "Usage: $0 [command]"
    echo ""
    echo "Commands:"
    echo "  release    - Full release: run tests, publish to crates.io, and push git tag"
    echo "  publish    - Publish to crates.io only"
    echo "  tag        - Create and push git tag only"
    echo "  check      - Run pre-release checks (tests, clippy, fmt)"
    echo "  build      - Build release binaries locally"
    echo "  help       - Show this help message"
    echo ""
    echo "Current version: v${VERSION}"
}

log_info() {
    echo -e "${BLUE}ℹ️  $1${NC}"
}

log_success() {
    echo -e "${GREEN}✅ $1${NC}"
}

log_warn() {
    echo -e "${YELLOW}⚠️  $1${NC}"
}

log_error() {
    echo -e "${RED}❌ $1${NC}"
}

# Pre-release checks
run_checks() {
    echo ""
    echo "🔍 Running pre-release checks..."
    echo "================================"
    
    log_info "Checking formatting..."
    cargo fmt --check || { log_error "Format check failed. Run 'cargo fmt' first."; exit 1; }
    log_success "Format OK"
    
    log_info "Running clippy..."
    cargo clippy -- -D warnings || { log_error "Clippy found issues."; exit 1; }
    log_success "Clippy OK"
    
    log_info "Running tests..."
    cargo test || { log_error "Tests failed."; exit 1; }
    log_success "Tests OK"
    
    log_info "Checking package..."
    cargo package --allow-dirty --list > /dev/null || { log_error "Package check failed."; exit 1; }
    log_success "Package OK"
    
    echo ""
    log_success "All checks passed!"
}

# Publish to crates.io
publish_crates() {
    echo ""
    echo "📦 Publishing to crates.io..."
    echo "================================"
    
    # Check if already published
    if cargo search "$BINARY_NAME" 2>/dev/null | grep -q "^$BINARY_NAME = \"$VERSION\""; then
        log_warn "Version $VERSION already published to crates.io"
        return 0
    fi
    
    log_info "Publishing $BINARY_NAME v$VERSION to crates.io..."
    cargo publish || { log_error "Failed to publish to crates.io"; exit 1; }
    
    log_success "Published to crates.io!"
}

# Create and push git tag
push_tag() {
    echo ""
    echo "🏷️  Creating git tag..."
    echo "================================"
    
    local tag="v${VERSION}"
    
    # Check for uncommitted changes
    if ! git diff-index --quiet HEAD -- 2>/dev/null; then
        log_error "You have uncommitted changes. Please commit or stash them first."
        exit 1
    fi
    
    # Check if tag already exists locally
    if git tag -l | grep -q "^${tag}$"; then
        log_warn "Tag $tag already exists locally"
        read -p "Delete and recreate? (y/N) " -n 1 -r
        echo
        if [[ $REPLY =~ ^[Yy]$ ]]; then
            git tag -d "$tag"
            git push origin ":refs/tags/$tag" 2>/dev/null || true
        else
            log_info "Skipping tag creation"
            return 0
        fi
    fi
    
    log_info "Creating tag $tag..."
    git tag -a "$tag" -m "Release $tag"
    
    log_info "Pushing tag to origin..."
    git push origin "$tag"
    
    log_success "Tag $tag pushed! GitHub Actions will build and release."
    echo ""
    echo "🔗 Check release progress at:"
    echo "   https://github.com/Staninbui/racoon/actions"
}

# Full release process
full_release() {
    echo ""
    echo "🦝 Releasing $BINARY_NAME v${VERSION}"
    echo "========================================"
    
    # Confirm
    echo ""
    log_warn "This will:"
    echo "  1. Run all checks (fmt, clippy, tests)"
    echo "  2. Publish to crates.io"
    echo "  3. Create and push git tag v${VERSION}"
    echo "  4. Trigger GitHub Actions to build releases"
    echo ""
    read -p "Continue? (y/N) " -n 1 -r
    echo
    if [[ ! $REPLY =~ ^[Yy]$ ]]; then
        log_info "Aborted."
        exit 0
    fi
    
    run_checks
    publish_crates
    push_tag
    
    echo ""
    echo "========================================"
    log_success "Release v${VERSION} complete!"
    echo ""
    echo "📦 crates.io: https://crates.io/crates/$BINARY_NAME"
    echo "🐙 GitHub:    https://github.com/Staninbui/racoon/releases/tag/v${VERSION}"
}

# Build local release binaries
build_local() {
    local DIST_DIR="dist/v${VERSION}"
    
    echo ""
    echo "🔨 Building local release binaries..."
    echo "================================"
    
    mkdir -p "$DIST_DIR"
    
    build_target() {
        local target=$1
        local ext=$2
        local archive_ext=$3
        local use_cross=${4:-false}

        log_info "Building for $target..."

        if [ "$use_cross" = "true" ]; then
            if ! command -v cross &> /dev/null; then
                log_warn "cross not installed, skipping $target"
                return 1
            fi
            if ! docker info &> /dev/null; then
                log_warn "Docker not running, skipping $target"
                return 1
            fi
            cross build --release --target "$target" --features vendored-openssl
        else
            cargo build --release --target "$target"
        fi

        local binary="target/${target}/release/${BINARY_NAME}${ext}"
        local archive_name="${BINARY_NAME}-${target}${archive_ext}"

        if [ -f "$binary" ]; then
            if [ "$archive_ext" = ".zip" ]; then
                zip -j "${DIST_DIR}/${archive_name}" "$binary"
            else
                tar -czvf "${DIST_DIR}/${archive_name}" -C "target/${target}/release" "${BINARY_NAME}${ext}"
            fi
            log_success "Created ${DIST_DIR}/${archive_name}"
        else
            log_error "Failed to build for $target"
            return 1
        fi
    }

    # Detect OS and build appropriate targets
    if [[ "$OSTYPE" == "darwin"* ]]; then
        log_info "Building macOS targets..."
        rustup target add aarch64-apple-darwin 2>/dev/null || true
        rustup target add x86_64-apple-darwin 2>/dev/null || true
        
        build_target "aarch64-apple-darwin" "" ".tar.gz" false
        build_target "x86_64-apple-darwin" "" ".tar.gz" false
        
        echo ""
        log_info "Building Linux targets (requires Docker + cross)..."
        build_target "x86_64-unknown-linux-gnu" "" ".tar.gz" true || true
        build_target "aarch64-unknown-linux-gnu" "" ".tar.gz" true || true

    elif [[ "$OSTYPE" == "linux-gnu"* ]]; then
        log_info "Building Linux targets..."
        build_target "x86_64-unknown-linux-gnu" "" ".tar.gz" false
        build_target "aarch64-unknown-linux-gnu" "" ".tar.gz" true || true

    elif [[ "$OSTYPE" == "msys"* ]] || [[ "$OSTYPE" == "cygwin"* ]]; then
        log_info "Building Windows target..."
        build_target "x86_64-pc-windows-msvc" ".exe" ".zip" false
    fi

    echo ""
    log_success "Build complete!"
    echo "📁 Distribution files in: $DIST_DIR"
    ls -la "$DIST_DIR" 2>/dev/null || echo "(no files yet)"
}

# Main
case "${1:-help}" in
    release)
        full_release
        ;;
    publish)
        publish_crates
        ;;
    tag)
        push_tag
        ;;
    check)
        run_checks
        ;;
    build)
        build_local
        ;;
    help|--help|-h)
        usage
        ;;
    *)
        log_error "Unknown command: $1"
        usage
        exit 1
        ;;
esac