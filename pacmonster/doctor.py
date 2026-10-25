import os
from pathlib import Path
import shutil
import subprocess
import sys

from .cache import CACHE_DIR, load_inventory_cache, load_status_cache
from .discovery import get_host_distro, get_package_manager
from .librarian import SNAPSHOT_ROOT, SnapshotError, list_snapshots

PACKAGE_HEALTH_COMMANDS = {
    "apt": ["apt-get", "check"],
    "pacman": ["pacman", "-Dk"],
    "dnf": ["dnf", "check"],
}
PACKAGE_LOCK_PATHS = {
    "apt": (),
    "pacman": ("/var/lib/pacman/db.lck",),
    "dnf": (),
}
PACKAGE_PROCESS_NAMES = {
    "apt": {"apt", "apt-get", "dpkg", "unattended-upgrade", "packagekitd"},
    "pacman": {"pacman", "yay", "paru", "pamac"},
    "dnf": {"dnf", "dnf5", "rpm", "packagekitd"},
}


def command_exists(cmd):
    """Return whether a command is available through the current PATH."""

    return shutil.which(cmd) is not None


def active_package_processes(package_manager):
    """Return matching package-manager processes reported by ``ps``."""

    names = PACKAGE_PROCESS_NAMES.get(package_manager, set())
    if not names:
        return []

    result = subprocess.run(
        ["ps", "-eo", "comm="],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise OSError(result.stderr.strip() or "could not inspect running processes")
    return sorted({name for name in result.stdout.splitlines() if name in names})


def run_doctor():
    """Diagnose privileges, native tools, caches, locks, storage, and snapshots."""

    checks = []

    def add(name, status, detail):
        """Append one normalized diagnostic result."""

        checks.append({"name": name, "status": status, "detail": detail})

    add(
        "root privileges",
        "ok" if os.geteuid() == 0 else "warning",
        "running as root" if os.geteuid() == 0 else "run with sudo for package operations",
    )

    distro = get_host_distro()
    package_manager = get_package_manager(distro)
    lock_paths = PACKAGE_LOCK_PATHS.get(package_manager, ())
    existing_locks = [path for path in lock_paths if Path(path).exists()]
    try:
        active_processes = active_package_processes(package_manager)
    except OSError as error:
        active_processes = None
        add("package-manager process check", "warning", str(error))
    if package_manager == "unknown":
        add("host package manager", "warning", f"unsupported distribution: {distro}")
    else:
        available = command_exists(package_manager)
        add(
            "host package manager",
            "ok" if available else "fail",
            f"{package_manager} {'available' if available else 'not found'}",
        )
        if not available:
            add(
                "package database health",
                "skip",
                f"cannot check; {package_manager} is not installed",
            )
        elif active_processes is None:
            add(
                "package database health",
                "skip",
                "not run because running package-manager processes could not be checked",
            )
        elif existing_locks or active_processes:
            add(
                "package database health",
                "skip",
                "not run because package manager may be active: "
                + ", ".join([*existing_locks, *active_processes]),
            )
        elif os.geteuid() != 0:
            add(
                "package database health",
                "skip",
                "run doctor with sudo for package database checks",
            )
        else:
            health_command = PACKAGE_HEALTH_COMMANDS[package_manager]
            try:
                health = subprocess.run(
                    health_command,
                    capture_output=True,
                    text=True,
                    check=False,
                    timeout=60,
                )
                detail = (health.stdout + health.stderr).strip()
                add(
                    "package database health",
                    "ok" if health.returncode == 0 else "fail",
                    detail or (
                        f"{package_manager} consistency check completed"
                        if health.returncode == 0
                        else f"{package_manager} check exited with {health.returncode}"
                    ),
                )
            except (OSError, subprocess.TimeoutExpired) as error:
                add("package database health", "fail", str(error))

    is_proxmox = Path("/etc/pve").exists()
    for command in ("pct", "qm"):
        available = command_exists(command)
        if is_proxmox and not available:
            status = "fail"
            detail = f"{command} is required on Proxmox but was not found"
        elif is_proxmox:
            status = "ok"
            detail = f"{command} available"
        elif available:
            status = "ok"
            detail = f"{command} available"
        else:
            status = "skip"
            detail = f"not required on this non-Proxmox host"
        add(f"{command} command", status, detail)

    for name, loader in (
        ("inventory cache", load_inventory_cache),
        ("status cache", load_status_cache),
    ):
        try:
            present = loader() is not None
            add(
                name,
                "ok" if present else "warning",
                "available" if present else "not created yet; run the matching refresh command",
            )
        except (OSError, ValueError) as error:
            add(name, "fail", f"cannot read cache: {error}")

    lock_path = CACHE_DIR / "update.lock"
    add(
        "Pacmonster update lock",
        "warning" if lock_path.exists() else "ok",
        f"lock exists at {lock_path}" if lock_path.exists() else "no update lock present",
    )

    try:
        if os.geteuid() == 0:
            SNAPSHOT_ROOT.mkdir(parents=True, exist_ok=True)
        writable = SNAPSHOT_ROOT.is_dir() and os.access(SNAPSHOT_ROOT, os.W_OK)
        snapshots = list_snapshots()
        if snapshots and not snapshots[0].get("restorable"):
            add(
                "latest package snapshot",
                "warning",
                "package manifest is saved, but no cached package archives are available",
            )
        elif snapshots:
            add(
                "latest package snapshot",
                "ok",
                f"cached package archives are available for {snapshots[0]['target']['name']}",
            )
        else:
            add(
                "latest package snapshot",
                "warning",
                "no snapshots yet; run an update or `pacmonster librarian snapshot`",
            )
        add(
            "package snapshot store",
            "ok" if writable else "fail" if os.geteuid() == 0 else "warning",
            f"{len(snapshots)} snapshot(s) in {SNAPSHOT_ROOT}"
            if writable else f"not writable: {SNAPSHOT_ROOT}; run with sudo",
        )
    except (OSError, SnapshotError) as error:
        status = "warning" if os.geteuid() != 0 and isinstance(error, OSError) else "fail"
        add("package snapshot store", status, f"cannot access {SNAPSHOT_ROOT}: {error}")

    try:
        free_bytes = shutil.disk_usage(SNAPSHOT_ROOT if SNAPSHOT_ROOT.exists() else "/").free
        free_gib = free_bytes / (1024 ** 3)
        status = "ok" if free_gib >= 2 else "warning"
        add("snapshot disk space", status, f"{free_gib:.1f} GiB free")
    except OSError as error:
        add("snapshot disk space", "fail", str(error))

    lock_details = [*existing_locks, *(active_processes or [])]
    add(
        "native package-manager activity",
        "warning" if lock_details else "ok",
        "check active processes or lock files: " + ", ".join(lock_details)
        if lock_details else "no known package-manager process or lock file",
    )

    return checks


def run_installation_checks():
    """Check that the Python package, CLI entry point, and TUI are installed."""

    checks = []

    def add(name, status, detail):
        checks.append({"name": name, "status": status, "detail": detail})

    package_path = Path(__file__).resolve()
    add(
        "Pacmonster Python package",
        "ok" if package_path.is_file() else "fail",
        f"loaded from {package_path}",
    )
    add(
        "Python interpreter",
        "ok" if Path(sys.executable).is_file() else "fail",
        sys.executable,
    )

    cli_path = shutil.which("pacmonster")
    add(
        "Pacmonster CLI entry point",
        "ok" if cli_path else "warning",
        cli_path or "not found on PATH; run the installed CLI through its absolute path",
    )

    tui_path = shutil.which("pacmonster-tui")
    if tui_path:
        add("Pacmonster TUI", "ok", tui_path)
    else:
        add(
            "Pacmonster TUI",
            "warning",
            "not found on PATH; build it with `cd tui && cargo build` or run the installer",
        )

    return checks
