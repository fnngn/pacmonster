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
            command = "apt upgrade"
        else:
            command = "pacman -Syu"

        actions.append({
            "id": item["id"],
            "name": item["name"],
            "command": command,
            "state": item["state"],
            "updates": item["updates"],
            "packages": item.get("packages", [])
        })

    return actions


def perform_update():

    if not acquire_lock():
        return "LOCKED"

    try:
        return build_dry_run()
    finally:
        release_lock()
