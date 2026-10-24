#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
RUST_HOME="/usr/local/rustup"
CARGO_HOME="/usr/local/cargo"

if [[ "$EUID" -ne 0 ]]; then
    exec sudo -- "$SCRIPT_DIR/install.sh" "$@"
fi

export RUSTUP_HOME="$RUST_HOME"
export CARGO_HOME
export PATH="$CARGO_HOME/bin:$PATH"

ensure_native_build_tools() {
    if command -v cc >/dev/null 2>&1; then
        return
    fi

    local distro_id
    distro_id="$("$PYTHON_BIN" -c '
import re
from pathlib import Path
match = re.search(r"^ID=(.*)$", Path("/etc/os-release").read_text(), re.MULTILINE)
print(match.group(1).strip("\"") if match else "unknown")
')"
    case "$distro_id" in
        debian|ubuntu|proxmox)
            apt-get update
            apt-get install -y build-essential
            ;;
        arch|archarm)
            pacman -S --needed --noconfirm base-devel
            ;;
        fedora)
            dnf install -y gcc
            ;;
        *)
            echo "A C compiler is required to link the Rust TUI; install one and rerun install.sh." >&2
            exit 1
            ;;
    esac

    if ! command -v cc >/dev/null 2>&1; then
        echo "Native build tools were installed, but the C compiler 'cc' is still unavailable." >&2
        exit 1
    fi
}

ensure_rust_toolchain() {
    if command -v rustc >/dev/null 2>&1 && command -v cargo >/dev/null 2>&1; then
        if rustc --version | awk '
            { split($2, version, "."); exit !(version[1] > 1 || (version[1] == 1 && version[2] >= 85)) }
        ' && cargo --version | awk '
            { split($2, version, "."); exit !(version[1] > 1 || (version[1] == 1 && version[2] >= 85)) }
        '; then
            return
        fi
    fi

    local rustup_installer="$BUILD_ROOT/rustup-init.sh"
    if ! "$PYTHON_BIN" - "$rustup_installer" <<'PY'
import sys
import urllib.request

urllib.request.urlretrieve("https://sh.rustup.rs", sys.argv[1])
PY
    then
        echo "Cannot download the official Rust toolchain installer." >&2
        exit 1
    fi
    RUSTUP_HOME="$RUSTUP_HOME" CARGO_HOME="$CARGO_HOME" \
        sh "$rustup_installer" -y --no-modify-path --profile minimal --default-toolchain stable

    if ! "$CARGO_HOME/bin/rustc" --version | awk '
        { split($2, version, "."); exit !(version[1] > 1 || (version[1] == 1 && version[2] >= 85)) }
    '; then
        echo "Rust 1.85 or newer is required to build the Pacmonster TUI." >&2
        exit 1
    fi
}

PYTHON_BIN="$(command -v python3)"
SITE_PACKAGES="$("$PYTHON_BIN" -c 'import sysconfig; print(sysconfig.get_path("purelib", vars={"base": "/usr/local", "platbase": "/usr/local"}))')"
BUILD_ROOT="$(mktemp -d /tmp/pacmonster-install.XXXXXX)"
trap 'rm -rf -- "$BUILD_ROOT"' EXIT
BUILD_SOURCE="$BUILD_ROOT/source"
mkdir -p "$BUILD_SOURCE"
cp -a \
    "$SCRIPT_DIR/pacmonster" \
    "$SCRIPT_DIR/pacmonster.egg-info" \
    "$SCRIPT_DIR/pyproject.toml" \
    "$SCRIPT_DIR/README.md" \
    "$SCRIPT_DIR/LICENSE" \
    "$BUILD_SOURCE/"
EXPECTED_VERSION="$("$PYTHON_BIN" -c 'import pathlib, tomllib, sys; print(tomllib.loads(pathlib.Path(sys.argv[1]).read_text())["project"]["version"])' "$BUILD_SOURCE/pyproject.toml")"

"$PYTHON_BIN" -m pip install \
    --no-cache-dir \
    --break-system-packages \
    --prefix=/usr/local \
    --root-user-action=ignore \
    "$BUILD_SOURCE"

if [[ ! -d "$SITE_PACKAGES/pacmonster" ]]; then
    echo "Installation failed: Pacmonster was not installed to $SITE_PACKAGES." >&2
    exit 1
fi

INSTALLED_VERSION="$(PYTHONPATH="$SITE_PACKAGES" "$PYTHON_BIN" -c 'from importlib.metadata import version; print(version("pacmonster"))')"
if [[ "$INSTALLED_VERSION" != "$EXPECTED_VERSION" ]]; then
    echo "Installation failed: expected Pacmonster $EXPECTED_VERSION, found $INSTALLED_VERSION." >&2
    exit 1
fi

cat > /usr/local/bin/pacmonster <<EOF
#!$PYTHON_BIN
import sys
sys.path.insert(0, "$SITE_PACKAGES")
from pacmonster.main import main
if __name__ == "__main__":
    sys.exit(main())
EOF
chmod 755 /usr/local/bin/pacmonster

if ! /usr/local/bin/pacmonster help >/dev/null; then
    echo "Installation failed: the Pacmonster command could not be started." >&2
    exit 1
fi

ensure_native_build_tools
ensure_rust_toolchain
CARGO_BIN="$(command -v cargo)"
CARGO_TARGET_DIR="$BUILD_ROOT/tui-target" "$CARGO_BIN" build \
    --locked \
    --release \
    --manifest-path "$SCRIPT_DIR/tui/Cargo.toml"
install -m 755 "$BUILD_ROOT/tui-target/release/pacmonster-tui" \
    /usr/local/bin/pacmonster-tui
if [[ -x "$CARGO_HOME/bin/cargo" ]]; then
    for tool in cargo rustc rustup; do
        if [[ -x "$CARGO_HOME/bin/$tool" ]]; then
            cat > "/usr/local/bin/$tool" <<EOF
#!/usr/bin/env sh
export RUSTUP_HOME="$RUST_HOME"
export CARGO_HOME="$CARGO_HOME"
export PATH="$CARGO_HOME/bin:\$PATH"
exec "$CARGO_HOME/bin/$tool" "\$@"
EOF
            chmod 755 "/usr/local/bin/$tool"
        fi
    done
fi
install -d -o root -g root -m 0750 /var/lib/pacmonster/snapshots/blobs

echo "Installed pacmonster $INSTALLED_VERSION. Run it with: sudo pacmonster"
echo "Installed Pacmonster TUI. Launch the update dashboard with: sudo pacmonster ghosthunter"
