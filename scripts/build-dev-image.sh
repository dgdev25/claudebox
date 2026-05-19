#!/usr/bin/env bash
# Build claudebox-dev-<version>.qcow2 from images/Dockerfile.dev.
#
# Requires: docker, running Docker daemon.
# Output:   images/output/claudebox-dev-<version>.qcow2
#
# Usage:
#   ./scripts/build-dev-image.sh          # version defaults to 0.1.0
#   ./scripts/build-dev-image.sh 0.2.0

set -euo pipefail

VERSION="${1:-0.1.0}"
REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
IMAGE_TAG="claudebox-dev:${VERSION}"
OUTPUT_DIR="${REPO_ROOT}/images/output"
OUTPUT="${OUTPUT_DIR}/claudebox-dev-${VERSION}.qcow2"
DISK_MB=3072   # 3 GB

mkdir -p "$OUTPUT_DIR"

echo "==> [1/4] Building rootfs Docker image: ${IMAGE_TAG}"
docker build \
    --platform linux/amd64 \
    -t "$IMAGE_TAG" \
    -f "${REPO_ROOT}/images/Dockerfile.dev" \
    "${REPO_ROOT}/images/"

echo "==> [2/4] Exporting rootfs tarball"
CONTAINER_ID=$(docker create --platform linux/amd64 "$IMAGE_TAG")
TARBALL="${TMPDIR:-/tmp}/claudebox-rootfs-$$.tar"
docker export "$CONTAINER_ID" -o "$TARBALL"
docker rm -f "$CONTAINER_ID" > /dev/null
echo "    rootfs tarball: $(du -sh "$TARBALL" | cut -f1)"

echo "==> [3/4] Creating disk image (${DISK_MB} MB) inside Docker"

# Write the inner build script to a temp file so Docker can COPY it in cleanly.
INNER_SCRIPT="${TMPDIR:-/tmp}/claudebox-build-disk-$$.sh"
cat > "$INNER_SCRIPT" << ENDSCRIPT
#!/bin/sh
set -ex

VERSION="\$1"
DISK_MB="\$2"

apk add --no-cache e2fsprogs qemu-img tar >/dev/null 2>&1

echo "  Extracting rootfs..."
mkdir -p /tmp/rootfs
tar -xf /rootfs.tar -C /tmp/rootfs 2>/dev/null || true

# Ensure required empty dirs exist (kernel mounts these at boot)
mkdir -p /tmp/rootfs/proc /tmp/rootfs/sys /tmp/rootfs/dev /tmp/rootfs/run /tmp/rootfs/tmp
chmod 1777 /tmp/rootfs/tmp

echo "  Creating raw disk (${DISK_MB} MB)..."
dd if=/dev/zero of=/tmp/disk.raw bs=1M count="\$DISK_MB"

echo "  Formatting ext4 and populating..."
# mke2fs -d copies the directory into the image without a loop mount.
mke2fs -t ext4 -F -d /tmp/rootfs /tmp/disk.raw

echo "  Converting raw -> qcow2 (compressed)..."
qemu-img convert -f raw -O qcow2 -c /tmp/disk.raw "/output/claudebox-dev-\${VERSION}.qcow2"

echo "  Image created."
ENDSCRIPT
chmod +x "$INNER_SCRIPT"

docker run --rm --privileged \
    --platform linux/amd64 \
    -v "${TARBALL}:/rootfs.tar:ro" \
    -v "${OUTPUT_DIR}:/output" \
    -v "${INNER_SCRIPT}:/build-disk.sh:ro" \
    alpine:3.21 \
    /bin/sh /build-disk.sh "$VERSION" "$DISK_MB"

rm -f "$TARBALL" "$INNER_SCRIPT"

echo "==> [4/4] Image ready"
ls -lh "$OUTPUT"
echo ""
echo "Test locally:"
echo "  claudebox start myapp.rvf --rootfs ${OUTPUT}"
echo ""
echo "Upload to GitHub releases:"
echo "  gh release create v${VERSION} '${OUTPUT}' --title 'claudebox-dev v${VERSION}' --notes 'Alpine 3.21 dev image: Node 22, Python 3.12, git, gcc'"
