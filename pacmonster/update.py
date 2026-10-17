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
            "kind": item.kind,
            "distro": item.distro.title(),
            "package_manager": item.package_manager,
            "command": command
        })

    return actions


def confirm_critical_updates(actions):

    return True


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
        failed = 0

        print()
        print("PACMONSTER UPDATE")
        print()

        print("STEP  TYPE  ID    NAME           DISTRO   PKG     PROGRESS")
        print("────  ────  ────  ─────────────  ──────   ──────  ──────────")

        for index, action in enumerate(actions, start=1):

            progress = max(1, int(index / total * 10))
            percent = int(index / total * 100)

            bar = "█" * progress + "░" * (10 - progress)

            kind = action["kind"].upper()

            print()
            print(
                f"{index}/{total:<3}  "
                f"{kind:<4}  "
                f"{str(action['id']):<4}  "
                f"{action['name']:<13}  "
                f"{action['distro']:<8}  "
                f"{action['package_manager']:<7}  "
                f"{bar} {percent}%"
            )

            started = time.perf_counter()
            result = run(action["command"])
            duration = time.perf_counter() - started

            print(f"Completed in {duration:.1f}s")

            if result.returncode != 0:
                failed += 1

            results.append({
                "id": action["id"],
                "name": action["name"],
                "success": result.returncode == 0,
                "returncode": result.returncode,
                "duration": round(duration, 1)
            })

        total_duration = time.perf_counter() - started_total

        print()
        print("────────────────────────────────────────────────────────────")
        print()
        print(f"UPDATED      {len(results) - failed}")
        print(f"FAILED       {failed}")
        print(f"DURATION     {total_duration:.1f}s")
        print()
        print("────────────────────────────────────────────────────────────")

        return results

    finally:
        release_lock()
