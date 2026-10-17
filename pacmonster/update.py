import subprocess

from .cache import load_status_cache
from .lock import acquire_lock
from .lock import release_lock


def run(cmd):
    return subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )


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
            command = "apt-get update && apt-get upgrade -y"
        else:
            command = f"pct exec {item['id']} -- pacman -Syu --noconfirm"

        actions.append({
            "id": item["id"],
            "name": item["name"],
            "command": command,
            "state": item["state"],
            "updates": item["updates"],
            "packages": item.get("packages", []),
            "critical": item.get("critical", False)
        })

    return actions


def confirm_critical_updates(actions):

    critical = [a for a in actions if a.get("critical", False)]

    if not critical:
        return True

    print()
    print("CRITICAL UPDATES DETECTED")
    print()

    for item in critical:
        print(f"{item['id']} {item['name']}")

        for package in item.get("packages", []):
            print(f"  - {package}")

        print()

    answer = input("Continue? [y/N] ").strip().lower()

    return answer == "y"


def perform_update():

    if not acquire_lock():
        return "LOCKED"

    try:

        actions = build_dry_run()

        if actions is None:
            return None

        if not confirm_critical_updates(actions):
            return "ABORTED"

        results = []

        for action in actions:

            result = run(action["command"])

            results.append({
                "id": action["id"],
                "name": action["name"],
                "success": result.returncode == 0,
                "returncode": result.returncode,
                "stdout": result.stdout[-2000:],
                "stderr": result.stderr[-2000:]
            })

        return results

    finally:
        release_lock()
