import math
import os
import signal
import subprocess
import sys
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
from .librarian import SnapshotError, create_snapshot, package_manifest

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

    def __init__(self, json_output=False):
        """Initialize process tracking for one update run."""

        self._lock = threading.Lock()
        self._processes = set()
        self._stopping = False
        self._json_output = json_output

    def execute(self, action):
        """Run one planned action and return its exit status and duration."""

        started = time.perf_counter()

        with self._lock:
            if self._stopping:
                raise CancelledError("Update run was interrupted")

            process_options = {
                "shell": True,
                "text": True,
                "start_new_session": True,
            }
            if self._json_output:
                process_options.update(stdout=sys.stderr, stderr=sys.stderr)
            process = subprocess.Popen(action["command"], **process_options)
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


def _kernel_version(item):
    """Return the running kernel for the host or an LXC target."""

    command = ["uname", "-r"]
    if item.kind == "lxc":
        command = ["pct", "exec", str(item.id), "--", *command]
    try:
        result = subprocess.run(
            command,
            text=True,
            capture_output=True,
            check=False,
            timeout=10,
        )
    except (OSError, subprocess.TimeoutExpired):
        return None
    if result.returncode != 0:
        return None
    return result.stdout.strip() or None


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
            "kernel": _kernel_version(item),
            "command": command
        })

    return actions


def confirm_critical_updates(actions, assume_yes=False, json_output=False):
    """Return whether to proceed, prompting unless assume_yes is enabled."""

    if assume_yes:
        return True

    output = sys.stderr if json_output else sys.stdout
    print(file=output)
    print(f"Targets to update: {len(actions)}", file=output)

    print("Continue update run? [y/N] ", end="", file=output, flush=True)
    answer = input()

    return answer.lower() == "y"

def _package_changes(before, after):
    """Describe installed package version changes between two manifests."""

    changes = []
    for name in sorted(before.keys() | after.keys()):
        old_version = before.get(name)
        new_version = after.get(name)
        if old_version == new_version:
            continue
        if old_version is None:
            change = "installed"
        elif new_version is None:
            change = "removed"
        else:
            change = "updated"
        changes.append({
            "name": name,
            "change": change,
            "previous_version": old_version,
            "current_version": new_version,
        })
    return changes


def perform_update(assume_yes=False, json_output=False):
    """Run planned updates in parallel and return results or a control status.

    The update lock is held for the entire run and released on every exit path.
    """

    if not acquire_lock():
        return "LOCKED"

    started_total = time.perf_counter()
    output = sys.stderr if json_output else sys.stdout

    try:

        actions = build_dry_run()

        if actions is None:
            return None

        if not confirm_critical_updates(actions, assume_yes, json_output):
            return "ABORTED"

        try:
            for action in actions:
                print(
                    f"[{action['name']}] saving package snapshot before update...",
                    file=output,
                    flush=True,
                )
                snapshot = create_snapshot(action)
                action["snapshot_id"] = snapshot["id"]
                action["before_packages"] = snapshot.get("packages")
                print(
                    f"[{action['name']}] package snapshot "
                    f"{snapshot['id']} saved "
                    f"({snapshot['package_count']} packages)",
                    file=output,
                )
                if not snapshot["restorable"]:
                    print(
                        f"[{action['name']}] warning: no cached package "
                        "archives; recovery may require repository access",
                        file=output,
                    )
        except SnapshotError as error:
            print(
                f"Update cancelled: could not create package snapshot: {error}",
                file=output,
            )
            return "SNAPSHOT_FAILED"

        worker_count = get_worker_count(len(actions))
        cpu_cores = effective_cpu_cores()
        pool = GhostPool(max_ghosts=worker_count, cpu_cores=cpu_cores)
        results = []
        failed = 0
        runner = ActionRunner(json_output=json_output)

        print(file=output)
        print("PACMONSTER UPDATE", file=output)
        print(
            f"Using {worker_count} worker(s) "
            f"for {cpu_cores:g} available CPU core(s); "
            "concurrency is capped to reserve system resources.",
            file=output,
        )
        print(file=output)

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
                        record = {
                            "id": action["id"],
                            "name": action["name"],
                            "kind": action.get("kind"),
                            "distro": action.get("distro"),
                            "package_manager": action.get("package_manager"),
                            "kernel": action.get("kernel"),
                            "snapshot_id": action["snapshot_id"],
                            "success": result["success"],
                            "returncode": result["returncode"],
                            "duration": result["duration"],
                            "packages": [],
                        }
                        before_packages = action.get("before_packages")
                        if isinstance(before_packages, dict):
                            try:
                                after_packages = package_manifest(action)
                                record["packages"] = _package_changes(
                                    before_packages,
                                    after_packages,
                                )
                            except (SnapshotError, OSError) as error:
                                record["package_report_error"] = str(error)
                        results.append(record)
                        emit_pool_event(pool)
            except KeyboardInterrupt:
                print("\nInterrupt received; stopping running updates...", file=output)
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

        print(file=output)
        print(f"UPDATED      {len(results) - failed}", file=output)
        print(f"FAILED       {failed}", file=output)
        print(f"DURATION     {total_duration:.1f}s", file=output)
        print(file=output)
        print("────────────────────────────────────────────────────────────", file=output)

        return results

    finally:
        release_lock()
