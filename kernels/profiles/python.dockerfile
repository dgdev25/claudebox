# kernels/profiles/python.dockerfile
# Installs Python at the version specified by $LANG_VERSION using deadsnakes
# PPA or pyenv for fine-grained version control.
# This fragment is appended into a multi-stage Dockerfile by kernels/build.sh.
# The FROM + ARG/ENV lines are injected by the assembler; only RUN instructions
# should live here.

RUN set -eux; \
    apt-get update && apt-get install -y --no-install-recommends \
        build-essential \
        libssl-dev \
        zlib1g-dev \
        libbz2-dev \
        libreadline-dev \
        libsqlite3-dev \
        libffi-dev \
        xz-utils \
    && rm -rf /var/lib/apt/lists/*; \
    PYENV_ROOT="/opt/pyenv"; \
    git clone --depth=1 "https://github.com/pyenv/pyenv.git" "${PYENV_ROOT}"; \
    PYENV_ROOT="${PYENV_ROOT}" "${PYENV_ROOT}/bin/pyenv" install "${LANG_VERSION}"; \
    PYENV_ROOT="${PYENV_ROOT}" "${PYENV_ROOT}/bin/pyenv" global "${LANG_VERSION}"; \
    ln -sf "${PYENV_ROOT}/shims/python" /usr/local/bin/python; \
    ln -sf "${PYENV_ROOT}/shims/python3" /usr/local/bin/python3; \
    ln -sf "${PYENV_ROOT}/shims/pip" /usr/local/bin/pip; \
    ln -sf "${PYENV_ROOT}/shims/pip3" /usr/local/bin/pip3; \
    python --version; \
    pip --version
