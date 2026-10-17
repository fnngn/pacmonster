from cache import load_status_cache
from lock import acquire_lock
from lock import release_lock


def build_dry_run():

    cache = load_status_cache()

    if cache is None:
        return None

    actions = []

    for item in cache["status"]:

        if item["state"] in ("STOP", "ERR"):
            continue

        if str(item["updates"]) == "0":
            continue

        if item["id"] == "host":
            cmd = "apt upgrade"
        else:
            cmd = "pacman -Syu"

        actions.append({
            "id": item["id"],
            "name": item["name"],
            "command": cmd,
            "state": item["state"],
            "updates": item["updates"]
        })

    return actions


def perform_update():

    if not acquire_lock():
        return "LOCKED"

    try:
        actions = build_dry_run()

        if actions is None:
            return None

        return actions

    finally:
        release_lock()
