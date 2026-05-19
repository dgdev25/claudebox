# kernels/profiles/go.dockerfile
# Installs Go at the version specified by $LANG_VERSION.
# This fragment is appended into a multi-stage Dockerfile by kernels/build.sh.
# The FROM + ARG/ENV lines are injected by the assembler; only RUN instructions
# should live here.

RUN set -eux; \
    ARCH="$(uname -m)"; \
    case "${ARCH}" in \
        x86_64)  GO_ARCH="amd64" ;; \
        aarch64) GO_ARCH="arm64" ;; \
        armv7l)  GO_ARCH="armv6l" ;; \
        *)        echo "Unsupported architecture: ${ARCH}" >&2; exit 1 ;; \
    esac; \
    TARBALL="go${LANG_VERSION}.linux-${GO_ARCH}.tar.gz"; \
    curl -fsSL "https://go.dev/dl/${TARBALL}" -o /tmp/go.tar.gz; \
    tar -C /opt -xzf /tmp/go.tar.gz; \
    rm /tmp/go.tar.gz; \
    ln -sf /opt/go/bin/go   /usr/local/bin/go; \
    ln -sf /opt/go/bin/gofmt /usr/local/bin/gofmt; \
    go version
