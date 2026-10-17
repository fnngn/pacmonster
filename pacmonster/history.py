import json
from pathlib import Path
from datetime import datetime


CACHE_DIR = Path.home() / ".cache" / "pacmonster"
HISTORY_FILE = CACHE_DIR / "history.json"


def load_history():

    if not HISTORY_FILE.exists():
        return []

    with open(HISTORY_FILE) as f:
        return json.load(f)


def add_history(event_type, details=None):

    CACHE_DIR.mkdir(parents=True, exist_ok=True)

    history = load_history()

    history.append({
        "timestamp": datetime.now().isoformat(),
        "type": event_type,
        "details": details or {}
    })

    history = history[-100:]

    with open(HISTORY_FILE, "w") as f:
        json.dump(history, f, indent=2)
