# kernels/profiles/node.dockerfile
# Installs Node.js at the version specified by $LANG_VERSION.
# This fragment is appended into a multi-stage Dockerfile by kernels/build.sh.
# The FROM + ARG/ENV lines are injected by the assembler; only RUN instructions
# should live here.

RUN set -eux; \
    ARCH="$(uname -m)"; \
    case "${ARCH}" in \
        x86_64)  NODE_ARCH="x64" ;; \
        aarch64) NODE_ARCH="arm64" ;; \
        armv7l)  NODE_ARCH="armv7l" ;; \
        *)        echo "Unsupported architecture: ${ARCH}" >&2; exit 1 ;; \
    esac; \
    TARBALL="node-v${LANG_VERSION}-linux-${NODE_ARCH}.tar.xz"; \
    curl -fsSL "https://nodejs.org/dist/v${LANG_VERSION}/${TARBALL}" -o /tmp/node.tar.xz; \
    mkdir -p /usr/local/lib/node; \
    tar -xJf /tmp/node.tar.xz -C /usr/local/lib/node --strip-components=1; \
    rm /tmp/node.tar.xz; \
    ln -sf /usr/local/lib/node/bin/node /usr/local/bin/node; \
    ln -sf /usr/local/lib/node/bin/npm /usr/local/bin/npm; \
    ln -sf /usr/local/lib/node/bin/npx /usr/local/bin/npx; \
    node --version; \
    npm --version
