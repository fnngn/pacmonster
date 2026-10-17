from cache import load_status_cache


def build_dry_run():

    cache = load_status_cache()

    if cache is None:
        return None

    actions = []

    for item in cache["status"]:

        if item["state"] in ("STOP", "ERR"):
            continue

        updates = item["updates"]

        if str(updates) == "0":
            continue

        if item["id"] == "host":
            command = "apt upgrade"
        else:
            command = "pacman -Syu"

        actions.append({
            "id": item["id"],
            "name": item["name"],
            "updates": updates,
            "command": command,
            "state": item["state"]
        })

    return actions
