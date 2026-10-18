import subprocess
import os
import time

from .cache import load_status_cache
from .lock import acquire_lock
from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import as_completed

from .lock import release_lock
from .events import emit_ghost_event
from .events import emit_pool_event
from .ghost_pool import GhostPool
GHOSTS = 4

def execute_action(action):

    started = time.perf_counter()

    result = subprocess.run(
        action["command"],
        shell=True,
        text=True,
        capture_output=True
    )

    duration = time.perf_counter() - started

    return {
        "action": action,
        "stdout": result.stdout,
        "stderr": result.stderr,
        "returncode": result.returncode,
        "success": result.returncode == 0,
        "duration": round(duration, 1)
    }


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


def confirm_critical_updates(actions, assume_yes=False):

    if assume_yes:
        return True

    print()
    print(f"Targets to update: {len(actions)}")

    answer = input("Continue update run? [y/N] ")

    return answer.lower() == "y"

def perform_update(assume_yes=False):

    if not acquire_lock():
        return "LOCKED"

    started_total = time.perf_counter()

    try:

        actions = build_dry_run()

        if actions is None:
            return None

        if not confirm_critical_updates(actions, assume_yes):
            return "ABORTED"

        pool = GhostPool(max_ghosts=GHOSTS)
        results = []
        total = len(actions)
        failed = 0

        print()
        print("PACMONSTER UPDATE")
        print()
        futures = {}

        with ThreadPoolExecutor(max_workers=GHOSTS) as ghosts:

            pending = list(actions)
            running = {}

            while pending or running:

                while pending and pool.has_capacity():

                    action = pending.pop(0)

                    ghost = pool.allocate(action["name"])

                    if ghost is not None:

                        emit_ghost_event(
                            "assigned",
                            ghost,
                            action["name"]
                        )

                        emit_pool_event(pool)



                    if ghost is None:
                        break

                    future = ghosts.submit(
                        execute_action,
                        action
                    )

                    running[future] = {
                        "ghost": ghost,
                        "action": action,
                    }

                if not running:
                    continue

                completed_future = next(
                    as_completed(running)
                )

                meta = running.pop(
                    completed_future
                )

                ghost = meta["ghost"]

                result = completed_future.result()

                emit_ghost_event(
                    "released",
                    ghost,
                    ghost.target
                )

                pool.release(ghost.id)

                emit_pool_event(pool)

                if result["stdout"]:
                    print(result["stdout"], end="")

                if result["stderr"]:
                    print(result["stderr"], end="")

                print(
                    f"Completed in {result['duration']:.1f}s"
                )

                if not result["success"]:
                    failed += 1

                action = result["action"]

                results.append({
                    "id": action["id"],
                    "name": action["name"],
                    "success": result["success"],
                    "returncode": result["returncode"],
                    "duration": result["duration"]
                })

        total_duration = time.perf_counter() - started_total

        print()
        print(f"UPDATED      {len(results) - failed}")
        print(f"FAILED       {failed}")
        print(f"DURATION     {total_duration:.1f}s")
        print()
        print("────────────────────────────────────────────────────────────")

        return results

    finally:
        release_lock()
