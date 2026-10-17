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

    from .inventory import build_inventory

    inventory = build_inventory()

    actions = []

    for item in inventory:

        if item.status != "running":
            continue

        if item.kind == "host":
            command = "apt-get update && apt-get upgrade -y"

        elif item.kind == "lxc" and item.package_manager == "pacman":
            command = f"pct exec {item.id} -- pacman -Syu --noconfirm"

        else:
            continue

        actions.append({
            "id": item.id,
            "name": item.name,
            "command": command,
            "state": "DIRECT",
            "updates": "?",
            "packages": [],
            "critical": False
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
