# kernels/profiles/rust.dockerfile
# Installs Rust toolchain at the version specified by $LANG_VERSION via rustup.
# This fragment is appended into a multi-stage Dockerfile by kernels/build.sh.
# The FROM + ARG/ENV lines are injected by the assembler; only RUN instructions
# should live here.

RUN set -eux; \
    curl -fsSL https://sh.rustup.rs -o /tmp/rustup-init.sh; \
    chmod +x /tmp/rustup-init.sh; \
    RUSTUP_HOME="/opt/rustup" \
    CARGO_HOME="/opt/cargo" \
    /tmp/rustup-init.sh -y \
        --no-modify-path \
        --default-toolchain "${LANG_VERSION}" \
        --profile minimal; \
    rm /tmp/rustup-init.sh; \
    ln -sf /opt/cargo/bin/rustup  /usr/local/bin/rustup; \
    ln -sf /opt/cargo/bin/cargo   /usr/local/bin/cargo; \
    ln -sf /opt/cargo/bin/rustc   /usr/local/bin/rustc; \
    ln -sf /opt/cargo/bin/rustfmt /usr/local/bin/rustfmt; \
    ln -sf /opt/cargo/bin/clippy-driver /usr/local/bin/clippy-driver; \
    rustc --version; \
    cargo --version
