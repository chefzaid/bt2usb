#!/usr/bin/env bash
#
# Install Renode (portable) + the Python dependencies for `renode-test` into the
# current user's home — no root required. Linux / WSL2 only. Idempotent: re-runs
# skip work that's already done.
#
# This automates the Layer-3 (on-target simulation) toolchain. After running it,
# `renode` and `renode-test` are on PATH (via ~/.local/bin) and you can:
#
#   mask sim-build && mask sim        # GUI
#   mask sim-build && mask sim-test   # headless / CI
#
# Override the version or install location with env vars:
#   RENODE_VERSION=1.16.1 RENODE_DIR=$HOME/.local/share/renode ./scripts/install-renode.sh
#
set -euo pipefail

RENODE_VERSION="${RENODE_VERSION:-1.16.1}"
RENODE_DIR="${RENODE_DIR:-$HOME/.local/share/renode}"
BIN_DIR="${BIN_DIR:-$HOME/.local/bin}"

PORTABLE="renode-${RENODE_VERSION}.linux-portable-dotnet.tar.gz"
URL="https://github.com/renode/renode/releases/download/v${RENODE_VERSION}/${PORTABLE}"
EXTRACT_DIR="${RENODE_DIR}/renode_${RENODE_VERSION}-dotnet_portable"

if [ "$(uname -s)" != "Linux" ]; then
    echo "ERROR: this installer targets Linux/WSL2." >&2
    echo "On Windows, run it inside WSL:  wsl -d Ubuntu -- ./scripts/install-renode.sh" >&2
    exit 1
fi

echo "==> Renode ${RENODE_VERSION} -> ${RENODE_DIR}"
mkdir -p "${RENODE_DIR}" "${BIN_DIR}"

if [ -x "${EXTRACT_DIR}/renode" ]; then
    echo "    already extracted, skipping download"
else
    echo "    downloading ${PORTABLE} ..."
    curl -fsSL -o "${RENODE_DIR}/${PORTABLE}" "${URL}"
    echo "    extracting ..."
    tar xzf "${RENODE_DIR}/${PORTABLE}" -C "${RENODE_DIR}"
    rm -f "${RENODE_DIR}/${PORTABLE}"
fi

# Thin wrappers on PATH that exec the real launchers by absolute path (so their
# own root-detection works regardless of how they're invoked).
echo "==> wrappers in ${BIN_DIR} (renode, renode-test)"
for tool in renode renode-test; do
    cat > "${BIN_DIR}/${tool}" <<EOF
#!/usr/bin/env bash
exec "${EXTRACT_DIR}/${tool}" "\$@"
EOF
    chmod +x "${BIN_DIR}/${tool}"
done

# `renode-test` is a Robot Framework harness needing Python deps. Debian/Ubuntu
# ship a stripped python3 (no pip), so bootstrap a user pip first.
echo "==> Python deps for renode-test"
if ! python3 -m pip --version >/dev/null 2>&1; then
    echo "    bootstrapping pip (get-pip.py, user) ..."
    curl -fsSL https://bootstrap.pypa.io/get-pip.py -o /tmp/get-pip.py
    python3 /tmp/get-pip.py --user --break-system-packages
    rm -f /tmp/get-pip.py
fi

# psutil from a prebuilt wheel (avoids needing a C toolchain); rest are pure-Python.
python3 -m pip install --user --break-system-packages --only-binary=:all: "psutil>=5.9.8"
python3 -m pip install --user --break-system-packages \
    "robotframework==6.1" "pyyaml==6.0.*" "robotframework-retryfailed==0.2.0" "telnetlib3==2.0.*"

echo ""
echo "==> done. Installed: ${EXTRACT_DIR}"
if ! command -v renode >/dev/null 2>&1; then
    echo ""
    echo "NOTE: ${BIN_DIR} is not on your PATH. Add it, e.g.:"
    echo "      echo 'export PATH=\"\$HOME/.local/bin:\$PATH\"' >> ~/.bashrc && source ~/.bashrc"
fi
echo ""
echo "Verify:  renode --version"
echo "Run:     mask sim-build && mask sim-test"
