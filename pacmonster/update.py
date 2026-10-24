import os
import signal
import subprocess
import threading
import time
from collections import deque
from concurrent.futures import CancelledError
from concurrent.futures import FIRST_COMPLETED
from concurrent.futures import ThreadPoolExecutor
from concurrent.futures import wait

from .lock import acquire_lock

from .lock import release_lock
from .events import emit_ghost_event
from .events import emit_pool_event
from .ghost_pool import GhostPool
from .librarian import SnapshotError, create_snapshot
GHOSTS = 4


class ActionRunner:
    """Run package-manager commands and stop their process groups on request."""

    def __init__(self):
        """Initialize process tracking for one update run."""

        self._lock = threading.Lock()
        self._processes = set()
        self._stopping = False

    def execute(self, action):
        """Run one planned action and return its exit status and duration."""

        started = time.perf_counter()

        with self._lock:
            if self._stopping:
                raise CancelledError("Update run was interrupted")

            process = subprocess.Popen(
                action["command"],
                shell=True,
                text=True,
                start_new_session=True
            )
            self._processes.add(process)

        try:
            returncode = process.wait()
        finally:
            with self._lock:
                self._processes.discard(process)

        duration = time.perf_counter() - started
        return {
            "action": action,
            "stdout": "",
            "stderr": "",
            "returncode": returncode,
            "success": returncode == 0,
            "duration": round(duration, 1)
        }

    def stop(self):
        """Stop active commands gracefully, escalating if they do not exit."""

        with self._lock:
            self._stopping = True
            processes = tuple(self._processes)

        for process in processes:
            try:
                os.killpg(process.pid, signal.SIGINT)
            except ProcessLookupError:
                pass

        self._wait_for_exit(processes, timeout=5)
        remaining = [process for process in processes if process.poll() is None]
        for process in remaining:
            try:
                os.killpg(process.pid, signal.SIGTERM)
            except ProcessLookupError:
                pass

        self._wait_for_exit(remaining, timeout=2)
        for process in remaining:
            if process.poll() is None:
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass

    @staticmethod
    def _wait_for_exit(processes, timeout):
        """Wait up to timeout seconds for all processes to exit."""

        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            if all(process.poll() is not None for process in processes):
                return
            time.sleep(0.1)


def execute_action(action, runner):
    """Execute one scheduler action through the shared action runner."""

    return runner.execute(action)


def build_dry_run():
    """Build package-update actions for supported running hosts and LXCs."""

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
    """Return whether to proceed, prompting unless assume_yes is enabled."""

    if assume_yes:
        return True

    print()
    print(f"Targets to update: {len(actions)}")

    answer = input("Continue update run? [y/N] ")

    return answer.lower() == "y"

def perform_update(assume_yes=False):
    """Run planned updates in parallel and return results or a control status.

    The update lock is held for the entire run and released on every exit path.
    """

    if not acquire_lock():
        return "LOCKED"

    started_total = time.perf_counter()

    try:

        actions = build_dry_run()

        if actions is None:
            return None

        if not confirm_critical_updates(actions, assume_yes):
            return "ABORTED"

        try:
            for action in actions:
                print(
                    f"[{action['name']}] saving package snapshot before update...",
                    flush=True,
                )
                snapshot = create_snapshot(action)
                action["snapshot_id"] = snapshot["id"]
                print(
                    f"[{action['name']}] package snapshot "
                    f"{snapshot['id']} saved "
                    f"({snapshot['package_count']} packages)"
                )
                if not snapshot["restorable"]:
                    print(
                        f"[{action['name']}] warning: no cached package "
                        "archives; recovery may require repository access"
                    )
        except SnapshotError as error:
            print(f"Update cancelled: could not create package snapshot: {error}")
            return "SNAPSHOT_FAILED"

        pool = GhostPool(max_ghosts=GHOSTS)
        results = []
        failed = 0
        runner = ActionRunner()

        print()
        print("PACMONSTER UPDATE")
        print()

        with ThreadPoolExecutor(max_workers=GHOSTS) as ghosts:
            pending = deque(actions)
            running = {}
            interrupted = False

            try:
                while pending or running:
                    while pending and pool.has_capacity():
                        action = pending.popleft()
                        ghost = pool.allocate(action["name"])

                        if ghost is None:
                            break

                        emit_ghost_event("assigned", ghost, action["name"])
                        emit_pool_event(pool)

                        future = ghosts.submit(execute_action, action, runner)
                        running[future] = {
                            "ghost": ghost,
                            "action": action,
                            "started": time.perf_counter(),
                        }

                    if not running:
                        continue

                    completed, _ = wait(
                        running,
                        timeout=10,
                        return_when=FIRST_COMPLETED
                    )

                    if not completed:
                        now = time.perf_counter()
                        for meta in running.values():
                            elapsed = now - meta["started"]
                            print(
                                f"[{meta['action']['name']}] still running "
                                f"({elapsed:.0f}s)",
                                flush=True
                            )
                        continue

                    for completed_future in completed:
                        meta = running.pop(completed_future)
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

                        print(f"Completed in {result['duration']:.1f}s")

                        if not result["success"]:
                            failed += 1

                        action = result["action"]
                        results.append({
                            "id": action["id"],
                            "name": action["name"],
                            "snapshot_id": action["snapshot_id"],
                            "success": result["success"],
                            "returncode": result["returncode"],
                            "duration": result["duration"]
                        })
            except KeyboardInterrupt:
                print("\nInterrupt received; stopping running updates...")
                pending.clear()
                for future in running:
                    future.cancel()
                runner.stop()
                interrupted = True

        if interrupted:
            for meta in running.values():
                ghost = meta["ghost"]
                emit_ghost_event("released", ghost, ghost.target)
                pool.release(ghost.id)
                emit_pool_event(pool)
            return "INTERRUPTED"

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
