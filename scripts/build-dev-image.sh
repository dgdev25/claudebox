#!/usr/bin/env bash
# Build claudebox-dev-<version>-<arch>.qcow2 from images/Dockerfile.dev.
#
# Requires: docker with buildx, running Docker daemon.
# Output:   images/output/claudebox-dev-<version>-<arch>.qcow2
#
# Usage:
#   ./scripts/build-dev-image.sh                    # host arch, version 0.1.0
#   ./scripts/build-dev-image.sh 0.2.0 aarch64      # explicit version + arch
#   ./scripts/build-dev-image.sh 0.1.0 x86_64

set -euo pipefail

VERSION="${1:-0.1.0}"
# Default to the host machine's architecture.
HOST_ARCH="$(uname -m)"
ARCH="${2:-${HOST_ARCH}}"

# Normalise arch to the form Docker/Linux expects.
case "$ARCH" in
  arm64|aarch64) ARCH="aarch64" ; DOCKER_PLATFORM="linux/arm64" ;;
  x86_64|amd64)  ARCH="x86_64"  ; DOCKER_PLATFORM="linux/amd64" ;;
  *) echo "Unknown arch: $ARCH (use aarch64 or x86_64)"; exit 1 ;;
esac

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE_TAG="claudebox-dev:${VERSION}-${ARCH}"
OUTPUT_DIR="${REPO_ROOT}/images/output"
OUTPUT="${OUTPUT_DIR}/claudebox-dev-${VERSION}-${ARCH}.qcow2"
DISK_MB=3072   # 3 GB

mkdir -p "$OUTPUT_DIR"

echo "==> [1/4] Building rootfs Docker image: ${IMAGE_TAG} (${DOCKER_PLATFORM})"
docker buildx build \
    --platform "$DOCKER_PLATFORM" \
    --load \
    -t "$IMAGE_TAG" \
    -f "${REPO_ROOT}/images/Dockerfile.dev" \
    "${REPO_ROOT}/images/"

echo "==> [2/4] Exporting rootfs tarball"
CONTAINER_ID=$(docker create --platform "$DOCKER_PLATFORM" "$IMAGE_TAG")
TARBALL="${TMPDIR:-/tmp}/claudebox-rootfs-$$.tar"
docker export "$CONTAINER_ID" -o "$TARBALL"
docker rm -f "$CONTAINER_ID" > /dev/null
echo "    rootfs tarball: $(du -sh "$TARBALL" | cut -f1)"

echo "==> [3/4] Creating disk image (${DISK_MB} MB) inside Docker"

INNER_SCRIPT="${TMPDIR:-/tmp}/claudebox-build-disk-$$.sh"
cat > "$INNER_SCRIPT" << ENDSCRIPT
#!/bin/sh
set -ex

VERSION="\$1"
DISK_MB="\$2"
ARCH="\$3"

apk add --no-cache e2fsprogs qemu-img tar >/dev/null 2>&1

echo "  Extracting rootfs..."
mkdir -p /tmp/rootfs
tar -xf /rootfs.tar -C /tmp/rootfs 2>/dev/null || true

mkdir -p /tmp/rootfs/proc /tmp/rootfs/sys /tmp/rootfs/dev /tmp/rootfs/run /tmp/rootfs/tmp
chmod 1777 /tmp/rootfs/tmp

echo "  Creating raw disk (\${DISK_MB} MB)..."
dd if=/dev/zero of=/tmp/disk.raw bs=1M count="\$DISK_MB"

echo "  Formatting ext4 and populating..."
mke2fs -t ext4 -F -d /tmp/rootfs /tmp/disk.raw

echo "  Converting raw -> qcow2 (compressed)..."
qemu-img convert -f raw -O qcow2 -c /tmp/disk.raw "/output/claudebox-dev-\${VERSION}-\${ARCH}.qcow2"

echo "  Image created."
ENDSCRIPT
chmod +x "$INNER_SCRIPT"

# Run the disk builder natively (no --platform) — it doesn't matter what arch
# mke2fs runs on, it just writes bytes into the raw file.
docker run --rm --privileged \
    -v "${TARBALL}:/rootfs.tar:ro" \
    -v "${OUTPUT_DIR}:/output" \
    -v "${INNER_SCRIPT}:/build-disk.sh:ro" \
    alpine:3.21 \
    /bin/sh /build-disk.sh "$VERSION" "$DISK_MB" "$ARCH"

rm -f "$TARBALL" "$INNER_SCRIPT"

echo "==> [4/4] Image ready"
ls -lh "$OUTPUT"
echo ""
echo "Test locally:"
echo "  claudebox start myapp.rvf --rootfs ${OUTPUT}"
echo ""
echo "Upload to GitHub releases:"
echo "  gh release create v${VERSION} '${OUTPUT}'"
