import subprocess
import time

from .cache import load_status_cache
from .lock import acquire_lock
from .lock import release_lock


def run(cmd):
    return subprocess.run(
        cmd,
        shell=True,
        text=True
    )
def build_dry_run():

    from .inventory import build_inventory

    inventory = build_inventory()

    actions = []

    for item in inventory:

        if item.status != "running":
            continue

        command = None

        if item.kind == "host":

            if item.package_manager == "apt":
                command = "apt-get update && apt-get upgrade -y"

            elif item.package_manager == "pacman":
                command = "pacman -Syu --noconfirm"

            elif item.package_manager == "dnf":
                command = "dnf upgrade -y"

        elif item.kind == "lxc":

            if item.package_manager == "pacman":
                command = f"pct exec {item.id} -- pacman -Syu --noconfirm"

            elif item.package_manager == "apt":
                command = f"pct exec {item.id} -- apt-get update && pct exec {item.id} -- apt-get upgrade -y"

            elif item.package_manager == "dnf":
                command = f"pct exec {item.id} -- dnf upgrade -y"

        if command is None:
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

    started_total = time.perf_counter()

    try:

        actions = build_dry_run()

        if actions is None:
            return None

        if not confirm_critical_updates(actions):
            return "ABORTED"

        results = []
        total = len(actions)

        print()
        print("╔══════════════════════════════════════╗")
        print("║         PACMONSTER UPDATE           ║")
        print("╚══════════════════════════════════════╝")

        for index, action in enumerate(actions, start=1):

            started = time.perf_counter()

            print()
            print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
            print(f"[{index}/{total}] {action['id']} {action['name']}")
            print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")

            result = run(action["command"])

            duration = time.perf_counter() - started

            print()

            if result.returncode == 0:
                print(f"✓ Completed in {duration:.1f}s")
            else:
                print(f"✗ Failed in {duration:.1f}s (rc={result.returncode})")

            results.append({
                "id": action["id"],
                "name": action["name"],
                "success": result.returncode == 0,
                "returncode": result.returncode,
                "duration": round(duration, 1)
            })

        total_duration = time.perf_counter() - started_total

        print()
        print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
        print(f"Total duration: {total_duration:.1f}s")
        print("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")

        return results

    finally:
        release_lock()
