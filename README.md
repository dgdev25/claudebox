# ClaudeBox

Per-project isolated VM environment for Claude Code workflows, packaged as a portable `.rvf` appliance.

## Install

### Option 1: Prebuilt binary (recommended)

```bash
curl -fsSL https://raw.githubusercontent.com/dgdev25/claudebox/main/install.sh | bash
claudebox setup
```

### Option 2: Build from source

```bash
git clone https://github.com/dgdev25/claudebox.git
cd claudebox
cargo build --release
./target/release/claudebox setup
```

## Quickstart

1. Initialize a project appliance:

```bash
cd /path/to/project
claudebox init myapp --kernel-from ~/.claudebox/kernels/$(uname -m)/kernel
```

`--lang` is optional. If omitted, claudebox auto-detects from project files (`package.json`, `pyproject.toml`, `requirements.txt`, `Cargo.toml`, `go.mod`).

2. Start the VM:

```bash
claudebox start myapp.rvf --workspace .
```

3. Check status / logs:

```bash
claudebox status myapp.rvf
claudebox logs myapp.rvf --follow
```

4. Stop the VM:

```bash
claudebox stop myapp.rvf
```

## Common Commands

```bash
# Snapshot lifecycle
claudebox snapshot myapp.rvf create pre-refactor
claudebox snapshot myapp.rvf list
claudebox snapshot myapp.rvf restore pre-refactor

# Branch / rollback
claudebox branch myapp.rvf feature-x
claudebox rollback myapp.rvf feature-x

# Audit and maintenance
claudebox audit myapp.rvf
claudebox migrate myapp.rvf
claudebox compact myapp.rvf

# Kernel ops
claudebox kernel myapp.rvf show
claudebox kernel myapp.rvf cache
claudebox upgrade-kernel myapp.rvf --kernel-from /path/to/bzImage
```

## Notes

- Default usage does **not** require Docker.
- Kernel provisioning defaults to prebuilt/cached kernels (`--kernel-from` or `claudebox setup` cache).
- Platform support is Linux and macOS.

## Releases

See [`docs/BINARY_RELEASES.md`](docs/BINARY_RELEASES.md) for maintainer release steps and manual install details.
