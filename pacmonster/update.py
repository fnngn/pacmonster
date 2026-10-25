import math
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
from .events import write_state
from .ghost_pool import GhostPool
from .librarian import SnapshotError, create_snapshot

MAX_UPDATE_WORKERS = 8
MIN_MEMORY_RESERVE_BYTES = 1024**3
MEMORY_PER_WORKER_BYTES = 1024**3


def _read_integer(path):
    try:
        with open(path) as file:
            value = file.read().strip()
    except OSError:
        return None
    if value == "max":
        return None
    try:
        return int(value)
    except ValueError:
        return None


def effective_cpu_cores():
    """Return the CPU capacity available to this process, including cgroups."""

    affinity = getattr(os, "sched_getaffinity", None)
    if affinity:
        try:
            available = float(max(1, len(affinity(0))))
        except OSError:
            available = float(max(1, os.cpu_count() or 1))
    else:
        available = float(max(1, os.cpu_count() or 1))

    for quota_path, period_path in (
        ("/sys/fs/cgroup/cpu.max", None),
        (
            "/sys/fs/cgroup/cpu/cpu.cfs_quota_us",
            "/sys/fs/cgroup/cpu/cpu.cfs_period_us",
        ),
        (
            "/sys/fs/cgroup/cpu,cpuacct/cpu.cfs_quota_us",
            "/sys/fs/cgroup/cpu,cpuacct/cpu.cfs_period_us",
        ),
    ):
        quota = _read_integer(quota_path)
        period = (
            _read_integer(period_path)
            if period_path is not None
            else None
        )
        if (
            quota is not None
            and 0 < quota < 2**60
            and period
            and period > 0
        ):
            available = min(available, quota / period)
            break
        if quota_path.endswith("cpu.max"):
            try:
                with open(quota_path) as file:
                    quota_text, period_text = file.read().split()
                if quota_text != "max":
                    available = min(
                        available,
                        int(quota_text) / int(period_text),
                    )
                    break
            except (OSError, ValueError, ZeroDivisionError):
                pass

    return max(1.0, available)


def available_memory_bytes():
    """Return memory available to Pacmonster, accounting for cgroup limits."""

    available = None
    try:
        with open("/proc/meminfo") as memory_info:
            for line in memory_info:
                if line.startswith("MemAvailable:"):
                    available = int(line.split()[1]) * 1024
                    break
    except (OSError, ValueError, IndexError):
        pass

    for limit_path, usage_path in (
        ("/sys/fs/cgroup/memory.max", "/sys/fs/cgroup/memory.current"),
        (
            "/sys/fs/cgroup/memory/memory.limit_in_bytes",
            "/sys/fs/cgroup/memory/memory.usage_in_bytes",
        ),
    ):
        limit = _read_integer(limit_path)
        usage = _read_integer(usage_path)
        if limit and usage is not None and limit < 2**60:
            cgroup_available = max(0, limit - usage)
            available = (
                cgroup_available
                if available is None
                else min(available, cgroup_available)
            )
            break
    return available


def get_worker_count(action_count):
    """Choose a conservative update concurrency from available system resources."""

    if action_count <= 0:
        return 1

    cores = effective_cpu_cores()
    worker_limit = min(
        MAX_UPDATE_WORKERS,
        max(1, math.floor(cores / 2)),
    )

    memory = available_memory_bytes()
    if memory is not None:
        memory_workers = max(
            1,
            (memory - MIN_MEMORY_RESERVE_BYTES) // MEMORY_PER_WORKER_BYTES,
        )
        worker_limit = min(worker_limit, memory_workers)

    try:
        load_average = os.getloadavg()[0]
    except (AttributeError, OSError):
        load_average = 0
    if load_average >= cores * 0.75:
        worker_limit = 1

    return min(action_count, worker_limit)


def estimate_action_seconds(action):
    """Estimate target duration from recent successful update history."""

    from .history import load_history

    durations = []
    for event in reversed(load_history()):
        if event.get("type") != "update":
            continue
        for target in event.get("details", {}).get("targets", []):
            if str(target.get("id")) == str(action.get("id")) and target.get(
                "success"
            ):
                try:
                    duration = float(target["duration"])
                except (KeyError, TypeError, ValueError):
                    continue
                if duration > 0:
                    durations.append(duration)
                    if len(durations) == 3:
                        return sum(durations) / len(durations)
    return sum(durations) / len(durations) if durations else None


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

        worker_count = get_worker_count(len(actions))
        cpu_cores = effective_cpu_cores()
        pool = GhostPool(max_ghosts=worker_count, cpu_cores=cpu_cores)
        results = []
        failed = 0
        runner = ActionRunner()

        print()
        print("PACMONSTER UPDATE")
        print(
            f"Using {worker_count} worker(s) "
            f"for {cpu_cores:g} available CPU core(s); "
            "concurrency is capped to reserve system resources."
        )
        print()

        with ThreadPoolExecutor(max_workers=worker_count) as ghosts:
            pending = deque(actions)
            running = {}
            interrupted = False

            try:
                while pending or running:
                    pool.set_limit(get_worker_count(len(actions)))
                    while pending and pool.has_capacity():
                        action = pending.popleft()
                        estimate = estimate_action_seconds(action)
                        ghost = pool.allocate(action["name"], estimate)

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
                        timeout=1,
                        return_when=FIRST_COMPLETED
                    )

                    if not completed:
                        now = time.perf_counter()
                        for meta in running.values():
                            elapsed = now - meta["started"]
                            pool.update_progress(meta["ghost"].id, elapsed)
                        write_state(pool.snapshot())
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
                        emit_pool_event(pool)
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
