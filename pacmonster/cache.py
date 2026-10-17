import json
from pathlib import Path
from datetime import datetime


CACHE_DIR = Path.home() / ".cache" / "pacmonster"
CACHE_FILE = CACHE_DIR / "inventory.json"


def load_cache():

    if not CACHE_FILE.exists():
        return None

    with open(CACHE_FILE) as f:
        return json.load(f)


def save_cache(data):

    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    payload = {
        "generated": datetime.now().isoformat(),
        "inventory": data
    }

    with open(CACHE_FILE, "w") as f:
        json.dump(payload, f, indent=2)


def cache_exists():
    return CACHE_FILE.exists()
