import json
from pathlib import Path
from datetime import datetime


CACHE_DIR = Path.home() / ".cache" / "pacmonster"

INVENTORY_CACHE = CACHE_DIR / "inventory.json"
STATUS_CACHE = CACHE_DIR / "status.json"


def load_inventory_cache():

    if not INVENTORY_CACHE.exists():
        return None

    with open(INVENTORY_CACHE) as f:
        return json.load(f)


def save_inventory_cache(data):

    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    with open(INVENTORY_CACHE, "w") as f:
        json.dump(data, f, indent=2)


def load_status_cache():

    if not STATUS_CACHE.exists():
        return None

    with open(STATUS_CACHE) as f:
        return json.load(f)


def save_status_cache(data):

    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    payload = {
        "generated": datetime.now().isoformat(),
        "status": data
    }

    with open(STATUS_CACHE, "w") as f:
        json.dump(payload, f, indent=2)
