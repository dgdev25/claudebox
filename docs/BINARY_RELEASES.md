# Binary Releases

ClaudeBox publishes prebuilt binaries from GitHub Actions when a version tag is pushed.

## For maintainers

1. Ensure `main` is green.
2. Tag and push:

```bash
git tag v0.1.0
git push origin v0.1.0
```

3. GitHub Actions workflow `.github/workflows/release.yml` builds:
- `claudebox-aarch64-apple-darwin`
- `claudebox-x86_64-apple-darwin`
- `claudebox-x86_64-unknown-linux-gnu`

4. Workflow uploads artifacts and `SHA256SUMS.txt` to the GitHub Release.
5. Release contract check: workflow fails unless `kernel-x86_64` and
   `initramfs-x86_64` are present in release assets.

## For users

Install latest binary:

```bash
curl -fsSL https://raw.githubusercontent.com/dgdev25/claudebox/main/install.sh | bash
claudebox setup
```

If installing from a fork/private repo:

```bash
CLAUDEBOX_REPO=owner/repo curl -fsSL https://raw.githubusercontent.com/owner/repo/main/install.sh | bash
```

Manual install from a specific release:

1. Download the matching file from GitHub Releases.
2. Make it executable and move to your PATH as `claudebox`.

Example (Linux x86_64):

```bash
chmod +x claudebox-x86_64-unknown-linux-gnu
sudo mv claudebox-x86_64-unknown-linux-gnu /usr/local/bin/claudebox
```
