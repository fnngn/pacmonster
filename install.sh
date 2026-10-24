#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ "$EUID" -ne 0 ]]; then
    exec sudo -- "$SCRIPT_DIR/install.sh" "$@"
fi

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

echo "Installed pacmonster $INSTALLED_VERSION. Run it with: sudo pacmonster"
