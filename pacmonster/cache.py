import json
from pathlib import Path
from datetime import datetime


CACHE_DIR = Path.home() / ".cache" / "pacmonster"

INVENTORY_CACHE = CACHE_DIR / "inventory.json"
STATUS_CACHE = CACHE_DIR / "status.json"


def load_inventory_cache():
    """Load the saved inventory, or return None when no cache exists."""

    if not INVENTORY_CACHE.exists():
        return None

    with open(INVENTORY_CACHE) as f:
        return json.load(f)


def save_inventory_cache(data):
    """Persist inventory data as formatted JSON in the user cache directory."""

    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    with open(INVENTORY_CACHE, "w") as f:
        json.dump(data, f, indent=2)


def load_status_cache():
    """Load the saved status snapshot, or return None when absent."""

    if not STATUS_CACHE.exists():
        return None

    with open(STATUS_CACHE) as f:
        return json.load(f)


def save_status_cache(data):
    """Save status data with its generation timestamp."""

    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    payload = {
        "generated": datetime.now().isoformat(),
        "status": data
    }

    with open(STATUS_CACHE, "w") as f:
        json.dump(payload, f, indent=2)
