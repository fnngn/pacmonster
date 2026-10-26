#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
RUST_HOME="/usr/local/rustup"
CARGO_HOME="/usr/local/cargo"
USE_PREBUILT_TUI=false

for argument in "$@"; do
    case "$argument" in
        --prebuilt-tui)
            USE_PREBUILT_TUI=true
            ;;
        --help|-h)
            printf 'Usage: %s [--prebuilt-tui]\n' "$0"
            printf '  --prebuilt-tui  Install a verified rolling-release binary instead of building locally.\n'
            exit 0
            ;;
        *)
            echo "Unknown installer option: $argument" >&2
            echo "Usage: $0 [--prebuilt-tui]" >&2
            exit 2
            ;;
    esac
done

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

download_release_tui() {
    local destination="$1"
    local release_tag="$2"
    local asset_arch
    case "$(uname -m)" in
        x86_64|amd64) asset_arch="x86_64" ;;
        aarch64|arm64) asset_arch="aarch64" ;;
        *) return 1 ;;
    esac

    "$PYTHON_BIN" - "$release_tag" "$asset_arch" "$destination" <<'PY'
import hashlib
import sys
import urllib.error
import urllib.request

release_tag, architecture, destination = sys.argv[1:]
repository = "fnngn/pacmonster"
filename = f"pacmonster-tui-linux-{architecture}"
base_url = f"https://github.com/{repository}/releases/download/{release_tag}"

try:
    with urllib.request.urlopen(f"{base_url}/{filename}", timeout=30) as response:
        binary = response.read()
    with urllib.request.urlopen(f"{base_url}/SHA256SUMS", timeout=30) as response:
        checksums = response.read().decode("ascii")
except urllib.error.HTTPError as error:
    if error.code == 404:
        sys.exit(1)
    print(f"Could not download the prebuilt Pacmonster TUI: {error}", file=sys.stderr)
    sys.exit(2)
except (OSError, UnicodeError) as error:
    print(f"Could not download the prebuilt Pacmonster TUI: {error}", file=sys.stderr)
    sys.exit(2)

expected = next(
    (line.split()[0] for line in checksums.splitlines()
     if len(line.split()) == 2 and line.split()[1] == filename),
    None,
)
actual = hashlib.sha256(binary).hexdigest()
if expected is None or actual != expected:
    print("The downloaded Pacmonster TUI failed its SHA-256 check.", file=sys.stderr)
    sys.exit(2)

with open(destination, "wb") as output:
    output.write(binary)
PY
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

TUI_BINARY="$BUILD_ROOT/pacmonster-tui"
if [[ "$USE_PREBUILT_TUI" == true ]]; then
    if download_release_tui "$TUI_BINARY" latest; then
        echo "Using the verified rolling-release TUI for $(uname -m)."
    else
        download_status=$?
        if [[ "$download_status" -eq 1 ]]; then
            echo "No prebuilt TUI is available for $(uname -m); omit --prebuilt-tui to build locally." >&2
            exit 1
        fi
        exit "$download_status"
    fi
    chmod 755 "$TUI_BINARY"
    TUI_VERSION="$("$TUI_BINARY" --version)"
    if [[ "$TUI_VERSION" != "pacmonster-tui $EXPECTED_VERSION" ]]; then
        echo "Prebuilt TUI version mismatch: expected $EXPECTED_VERSION, found $TUI_VERSION." >&2
        exit 1
    fi
else
    echo "Building the Pacmonster TUI from source."
    ensure_native_build_tools
    ensure_rust_toolchain
    CARGO_BIN="$(command -v cargo)"
    CARGO_TARGET_DIR="/var/cache/pacmonster/cargo-target"
    install -d -o root -g root -m 0755 "$CARGO_TARGET_DIR"
    CARGO_TARGET_DIR="$CARGO_TARGET_DIR" "$CARGO_BIN" build \
        --locked \
        --release \
        --manifest-path "$SCRIPT_DIR/tui/Cargo.toml"
    cp "$CARGO_TARGET_DIR/release/pacmonster-tui" "$TUI_BINARY"
fi
chmod 755 "$TUI_BINARY"
if ! "$TUI_BINARY" --version >/dev/null; then
    echo "Installation failed: the Pacmonster TUI binary cannot be started on this system." >&2
    exit 1
fi
install -m 755 "$TUI_BINARY" /usr/local/bin/pacmonster-tui
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
