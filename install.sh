#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

if [[ "$EUID" -ne 0 ]]; then
    exec sudo -- "$SCRIPT_DIR/install.sh" "$@"
fi

PYTHON_BIN="$(command -v python3)"
SITE_PACKAGES="$("$PYTHON_BIN" -c 'import sysconfig; print(sysconfig.get_path("purelib", vars={"base": "/usr/local", "platbase": "/usr/local"}))')"

"$PYTHON_BIN" -m pip install \
    --break-system-packages \
    --prefix=/usr/local \
    --root-user-action=ignore \
    "$SCRIPT_DIR"

if [[ ! -d "$SITE_PACKAGES/pacmonster" ]]; then
    echo "Installation failed: Pacmonster was not installed to $SITE_PACKAGES." >&2
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

echo "Installed. Run it with: sudo pacmonster"
