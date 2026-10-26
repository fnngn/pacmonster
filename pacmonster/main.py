import sys
import json
import os
import shutil
import subprocess
from contextlib import redirect_stdout
from datetime import datetime, timezone
from pathlib import Path

from .inventory import build_inventory
from .cache import load_inventory_cache
from .cache import load_status_cache
from .status import build_status
from .doctor import run_doctor
from .doctor import run_installation_checks
from .archeologist import search_logs
from .update import build_dry_run
from .update import perform_update
from .history import add_history
from .history import load_history
from .librarian import SnapshotError, create_snapshot, list_snapshots, load_snapshot
from .restorer import RestoreError, render_result, restore_snapshot


def render_inventory(data):
    """Print inventory records in aligned columns."""

    print()
    print("PACMONSTER INVENTORY")
    print()

    if not data:
        print("No inventory entries.")
        return

    id_width = max(len(str(x["id"])) for x in data)
    name_width = max(len(str(x["name"])) for x in data)
    distro_width = max(len(str(x["distro"])) for x in data)
    status_width = max(len(str(x["status"])) for x in data)

    print(
        "ID".ljust(id_width),
        "NAME".ljust(name_width),
        "DISTRO".ljust(distro_width),
        "STATUS".ljust(status_width)
    )

    print("-" * (id_width + name_width + distro_width + status_width + 6))

    for item in data:
        print(
            str(item["id"]).ljust(id_width),
            str(item["name"]).ljust(name_width),
            str(item["distro"]).ljust(distro_width),
            str(item["status"]).ljust(status_width)
        )


def render_status(data):
    """Print a compact status table for discovered systems."""

    print()
    print("PACMONSTER STATUS")
    print()

    if not data:
        print("No status entries.")
        return

    id_width = max(len(str(x["id"])) for x in data)
    name_width = max(len(str(x["name"])) for x in data)
    state_width = max(len(str(x["state"])) for x in data)

    print(
        "ID".ljust(id_width),
        "NAME".ljust(name_width),
        "STATE".ljust(state_width),
        "UPD"
    )

    print("-" * (id_width + name_width + state_width + 10))

    for item in data:
        print(
            str(item["id"]).ljust(id_width),
            str(item["name"]).ljust(name_width),
            str(item["state"]).ljust(state_width),
            item["updates"]
        )


def render_status_verbose(data):
    """Print detailed status information, including pending package names."""

    print()
    print("PACMONSTER STATUS (VERBOSE)")
    print()
    if not data:
        print("No status entries.")
        return


    for item in data:

        print(f"ID: {item['id']}")
        print(f"NAME: {item['name']}")
        print(f"STATE: {item['state']}")
        print(f"UPDATES: {item['updates']}")

        packages = item.get("packages", [])

        if packages:
            print("PACKAGES:")
            for package in packages:
                print(f"  - {package}")
        else:
            print("PACKAGES: -")

        print("-" * 25)


def refresh_command():
    """Rebuild and cache inventory, then record the refresh in history."""

    inventory = build_inventory()
    add_history("refresh", {"objects": len(inventory)})
    print(f"Inventory cache updated: {len(inventory)} objects")


def refresh_status_command():
    """Refresh inventory and package status, then record a history summary."""

    inventory = build_inventory()
    status = build_status(inventory)

    updates = 0
    critical = 0

    for item in status:
        if item["state"] == "UPD":
            updates += 1
        if item["state"] == "CRIT":
            critical += 1

    add_history(
        "status",
        {
            "objects": len(status),
            "systems_with_updates": updates,
            "critical_systems": critical
        }
    )

    print(f"Status cache updated: {len(status)} objects")


def inventory_command(json_output=False):
    """Display cached inventory as a table or JSON."""

    cache = load_inventory_cache()

    if cache is None:
        print("No inventory cache found.")
        return

    if json_output:
        print(json.dumps(cache["inventory"], indent=2))
        return

    render_inventory(cache["inventory"])

def status_command(verbose=False, json_output=False):
    """Display cached status in compact, verbose, or JSON form."""

    cache = load_status_cache()

    if cache is None:
        print("No status cache found.")
        return

    if json_output:
        print(json.dumps(cache["status"], indent=2))
        return

    if verbose:
        render_status_verbose(cache["status"])
    else:
        render_status(cache["status"])


def doctor_command(json_output=False, installation=False):
    """Run environment checks and display their results."""

    results = run_installation_checks() if installation else run_doctor()

    if json_output:
        print(json.dumps(results, indent=2))
    else:
        print()
        print(
            "PACMONSTER INSTALLATION CHECK"
            if installation
            else "PACMONSTER DOCTOR"
        )
        print()

        for check in results:
            print(
                f"[{check['status'].upper()}] {check['name']}: "
                f"{check['detail']}"
            )

    if any(check["status"] == "fail" for check in results):
        sys.exit(1)


def archeologist_command(command="all", query=None, json_output=False):
    """Search available application and package-manager logs."""

    if command not in {"all", "issues", "search"}:
        print("Usage: pacmonster archeologist [all|issues|search QUERY] [--json]")
        sys.exit(2)
    if command == "search" and not query:
        print("Usage: pacmonster archeologist search QUERY [--json]")
        sys.exit(2)

    records = search_logs(
        query=query if command == "search" else None,
        issues_only=command == "issues",
    )
    if json_output:
        print(json.dumps(records, indent=2))
        return
    if not records:
        print("No matching log entries found.")
        return
    for record in records:
        print(
            f"[{record['level'].upper()}] {record['timestamp']} "
            f"{record['source']}: {record['message']}"
        )


def librarian_command(command, target_id=None, json_output=False):
    """List, inspect, or create package snapshots."""

    if command == "list":
        try:
            snapshots = list_snapshots(target_id)
        except SnapshotError as error:
            print(f"Cannot list snapshots: {error}")
            sys.exit(1)
        if json_output:
            print(json.dumps(snapshots, indent=2))
            return
        if not snapshots:
            print("No package snapshots found.")
            return
        for snapshot in snapshots:
            archive_state = "archives saved" if snapshot["restorable"] else "manifest only"
            print(
                snapshot["id"],
                snapshot["target"]["name"],
                snapshot["created"],
                f"{snapshot['package_count']} packages",
                archive_state,
            )
        return

    if command == "show" and target_id:
        try:
            snapshot = load_snapshot(target_id)
        except SnapshotError as error:
            print(f"Snapshot unavailable: {error}")
            sys.exit(1)
        if json_output:
            print(json.dumps(snapshot, indent=2))
            return
        print(f"Snapshot: {snapshot['id']}")
        print(f"Target: {snapshot['target']['name']}")
        print(f"Created: {snapshot['created']}")
        print(f"Packages: {snapshot['package_count']}")
        print(
            f"Package archives: {snapshot['archive_bytes']} bytes referenced, "
            f"{snapshot.get('new_archive_bytes', snapshot['archive_bytes'])} bytes newly stored"
        )
        return

    if command != "snapshot":
        print("Usage: pacmonster librarian {list|snapshot [TARGET_ID]|show SNAPSHOT_ID}")
        sys.exit(2)

    inventory = build_inventory()
    targets = [
        item for item in inventory
        if item.kind in ("host", "lxc")
        and item.status == "running"
        and item.package_manager != "unknown"
        and (target_id is None or str(item.id) == str(target_id))
    ]
    if target_id is not None and not targets:
        print(f"No supported running host or container found: {target_id}")
        sys.exit(2)

    snapshots = []
    try:
        for target in targets:
            snapshot = create_snapshot(target.__dict__)
            snapshots.append(snapshot)
            if not json_output:
                print(
                    f"Saved {snapshot['id']}: {snapshot['package_count']} packages, "
                    f"{snapshot.get('new_archive_bytes', snapshot['archive_bytes'])} new archive bytes "
                    f"({snapshot['archive_bytes']} bytes referenced)"
                )
    except SnapshotError as error:
        print(f"Snapshot failed: {error}")
        sys.exit(1)

    if json_output:
        print(json.dumps(snapshots, indent=2))
    elif not targets:
        print("No supported running hosts or containers found.")


def restorer_command(command, snapshot_id=None, assume_yes=False, json_output=False):
    """List snapshots or restore a selected package snapshot."""

    if command == "list":
        try:
            snapshots = list_snapshots()
        except SnapshotError as error:
            print(f"Cannot list snapshots: {error}")
            sys.exit(1)
        if json_output:
            print(json.dumps(snapshots, indent=2))
        else:
            librarian_command("list")
        return
    if command != "restore" or not snapshot_id:
        print("Usage: pacmonster restorer {list|restore SNAPSHOT_ID [--yes]}")
        sys.exit(2)

    try:
        result = restore_snapshot(
            snapshot_id,
            assume_yes=assume_yes,
            json_output=json_output,
        )
    except (RestoreError, SnapshotError) as error:
        print(f"Restore failed: {error}")
        sys.exit(1)

    if json_output:
        print(json.dumps(result, indent=2))
    else:
        render_result(result)
    if result["status"] == "failed":
        sys.exit(1)
    if result["status"] == "missing":
        sys.exit(1)
    if result["status"] == "aborted":
        sys.exit(2)
    if result["status"] == "locked":
        sys.exit(3)


def dry_run_command(json_output=False):
    """Display planned package-manager commands without executing them."""

    actions = build_dry_run()

    if json_output:
        print(json.dumps(actions, indent=2))
        return

    print()
    print("PACMONSTER DRY RUN")
    print()

    if not actions:
        print("No actions required.")
        return

    for action in actions:
        print(
            f"{action['name']} (ID {action['id']}) | "
            f"{action['kind']} | {action['distro']} | "
            f"{action['package_manager']} | "
            f"Kernel: {action.get('kernel') or 'unavailable'}"
        )
        print(f"  Planned command: {action['command']}")


def _update_report(results):
    """Build the versioned JSON report shared by CLI and Ghosthunter."""

    systems = []
    for item in results:
        systems.append({
            "id": str(item["id"]),
            "name": item["name"],
            "kind": item.get("kind"),
            "distro": item.get("distro"),
            "package_manager": item.get("package_manager"),
            "kernel": item.get("kernel"),
            "success": bool(item.get("success", False)),
            "returncode": item.get("returncode"),
            "duration_seconds": item.get("duration", 0),
            "snapshot_id": item.get("snapshot_id"),
            "packages": item.get("packages", []),
            "package_report_error": item.get("package_report_error"),
        })

    updated_systems = [
        system for system in systems
        if system["success"] and system["packages"]
    ]
    failed_systems = [system for system in systems if not system["success"]]
    return {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "summary": {
            "systems_attempted": len(systems),
            "systems_succeeded": sum(system["success"] for system in systems),
            "systems_failed": len(failed_systems),
            "packages_changed": sum(
                len(system["packages"]) for system in systems
            ),
        },
        "updated_systems": updated_systems,
        "failed_systems": failed_systems,
        "systems": systems,
    }


def _print_update_report(report):
    """Print a readable summary of changed systems and package versions."""

    print()
    print("UPDATE REPORT")
    updated_systems = report["updated_systems"]
    if updated_systems:
        print("UPDATED SYSTEMS")
        for system in updated_systems:
            print(f"- {system['name']} (ID {system['id']})")
            for package in system["packages"]:
                previous = package.get("previous_version") or "not installed"
                current = package.get("current_version") or "removed"
                print(
                    f"  - {package['name']}: {previous} -> {current} "
                    f"({package['change']})"
                )
    else:
        print("UPDATED SYSTEMS: none (no package version changes detected)")

    failed_systems = report["failed_systems"]
    if failed_systems:
        print("FAILED SYSTEMS")
        for system in failed_systems:
            print(f"- {system['name']} (ID {system['id']})")
            for package in system["packages"]:
                previous = package.get("previous_version") or "not installed"
                current = package.get("current_version") or "removed"
                print(
                    f"  - {package['name']}: {previous} -> {current} "
                    f"({package['change']})"
                )
    for system in report["systems"]:
        if system.get("package_report_error"):
            print(
                f"Could not inspect packages on {system['name']}: "
                f"{system['package_report_error']}"
            )


def update_command(assume_yes=False, json_output=False):
    """Run updates and translate scheduler outcomes into CLI exit codes."""

    if json_output:
        with redirect_stdout(sys.stderr):
            result = perform_update(assume_yes, json_output=True)
    else:
        result = perform_update(assume_yes)

    errors = {
        "LOCKED": ("Another update is already running.", 3),
        "ABORTED": ("Update aborted.", 2),
        "INTERRUPTED": ("Update interrupted.", 130),
        "SNAPSHOT_FAILED": ("Could not create required package snapshots.", 4),
    }
    if isinstance(result, str) and result in errors:
        message, exit_code = errors[result]
        if json_output:
            print(json.dumps({
                "schema_version": 1,
                "status": "error",
                "error": result.lower(),
                "message": message,
            }))
        else:
            print(message)
        sys.exit(exit_code)

    if result is None:
        message = "No status cache found."
        if json_output:
            print(json.dumps({
                "schema_version": 1,
                "status": "error",
                "error": "missing_status_cache",
                "message": message,
            }))
        else:
            print(message)
        sys.exit(4)

    report = _update_report(result)

    updated = sum(1 for item in result if item.get("success", False))
    failed = len(result) - updated
    duration = round(sum(item.get("duration", 0) for item in result), 1)

    if json_output:
        print(json.dumps(report, indent=2))
    else:
        _print_update_report(report)

    add_history(
        "update",
        {
            "updated": updated,
            "failed": failed,
            "duration": duration,
            "snapshots": {
                item["id"]: item["snapshot_id"]
                for item in result
            },
            "targets": [
                {
                    "id": item["id"],
                    "name": item["name"],
                    "duration": item["duration"],
                    "success": item["success"],
                    "packages": item.get("packages", []),
                    "package_report_error": item.get("package_report_error"),
                }
                for item in result
            ],
        }
    )

    if failed > 0:
        sys.exit(1)

    sys.exit(0)


def _tui_command():
    """Find the installed or locally built Pacmonster TUI executable."""

    configured_path = os.environ.get("PACMONSTER_TUI")
    if configured_path:
        executable = Path(configured_path).expanduser()
        if executable.is_file() and os.access(executable, os.X_OK):
            return [str(executable)]
        raise FileNotFoundError(f"PACMONSTER_TUI is not an executable file: {executable}")

    installed = shutil.which("pacmonster-tui")
    if installed:
        return [installed]

    roots = (Path.cwd(), Path(__file__).resolve().parents[1])
    seen = set()
    for root in roots:
        target_roots = [root / "tui" / "target"]
        if (root / "Cargo.toml").is_file():
            target_roots.append(root / "target")
        for target_root in target_roots:
            for profile in ("debug", "release"):
                executable = target_root / profile / "pacmonster-tui"
                if executable in seen:
                    continue
                seen.add(executable)
                if executable.is_file() and os.access(executable, os.X_OK):
                    return [str(executable)]

    cargo = shutil.which("cargo")
    for root in roots:
        manifests = (root / "Cargo.toml", root / "tui" / "Cargo.toml")
        for manifest in manifests:
            if cargo and manifest.is_file():
                return [cargo, "run", "--quiet", "--manifest-path", str(manifest), "--"]
    raise FileNotFoundError(
        "Pacmonster TUI was not found. Build it with "
        "`cd tui && cargo build`, or install `pacmonster-tui` on PATH."
    )


def _run_tui(mode):
    """Launch the TUI in one of its privileged maintenance modes."""

    try:
        command = [*_tui_command(), f"--run-{mode}"]
    except FileNotFoundError as error:
        print(f"Cannot start {mode}: {error}", file=sys.stderr)
        sys.exit(4)

    try:
        result = subprocess.run(command, check=False)
    except OSError as error:
        print(f"Cannot start {mode} TUI: {error}", file=sys.stderr)
        sys.exit(4)
    if result.returncode != 0:
        sys.exit(result.returncode)


def ghosthunter_command(confirm=False):
    """Launch Ghosthunter, optionally retaining update confirmation prompts."""

    _run_tui("ghosthunter-confirm" if confirm else "ghosthunter")


def witchdoctor_command():
    """Launch the TUI and display structured system health checks."""

    _run_tui("doctor")


def start_command():
    """Launch the Pacmonster main menu."""

    _run_tui("start")



def history_command(json_output=False):
    """Display saved update and maintenance history."""

    history = load_history()

    if json_output:
        print(json.dumps(history, indent=2))
        return

    print()
    print("PACMONSTER HISTORY")
    print()

    for entry in reversed(history):
        print(entry["timestamp"], entry["type"])

        for key, value in entry.get("details", {}).items():
            print(" ", key + ":", value)

        print()
        print()
HELP_COMMANDS = [
    ("refresh", "Rediscover supported systems and refresh the inventory cache."),
    ("refresh-status", "Refresh inventory and cached package update status."),
    ("inventory [--json]", "Display the latest cached inventory."),
    ("status [--verbose] [--json]", "Display cached status; verbose includes package names."),
    ("doctor [--json]", "Diagnose system and package-manager health."),
    ("doctor --installation", "Check the Pacmonster CLI, Python package, and TUI."),
    ("dry-run [--json]", "Preview planned update commands without applying them."),
    ("update [--yes] [--json]", "Run package updates and optionally print a machine-readable report."),
    ("ghosthunter --confirm", "Run updates while retaining the confirmation prompt."),
    ("start", "Open the main menu for all Pacmonster interfaces."),
    ("ghosthunter", "Open the update dashboard and action menu."),
    ("witchdoctor", "Open diagnostics, installation checks, and status tools."),
    ("librarian", "Open snapshots, inventory, and update history."),
    ("archeologist", "Browse logs, find issues, or search log text."),
    ("restorer", "Open the snapshot recovery interface."),
    ("advisor", "Browse help, README, license, and developer information."),
    ("history [--json]", "Display recent update and maintenance history."),
    ("help [--json]", "Show this command reference as a table or JSON list."),
    ("librarian list [TARGET_ID]", "List snapshots, optionally filtered by target."),
    ("librarian snapshot [TARGET_ID]", "Create snapshots for all targets or one target."),
    ("librarian show SNAPSHOT_ID", "Display the contents of a saved snapshot."),
    ("archeologist all", "Display available Pacmonster and package-manager logs."),
    ("archeologist issues", "Show log records filtered to likely warnings and errors."),
    ("archeologist search QUERY", "Search available log records for text."),
    ("restorer list", "List snapshots that can be inspected for recovery."),
    ("restorer restore SNAPSHOT_ID [--yes]", "Restore package versions from a snapshot."),
]


def help_command(json_output=False):
    """Print the supported commands and a concise description of each."""

    commands = [command for command, _description in HELP_COMMANDS]
    if json_output:
        print(json.dumps(commands, indent=2))
    else:
        command_width = max(len(command) for command in commands)
        description_width = max(len(description) for _command, description in HELP_COMMANDS)
        separator = f"+-{'-' * command_width}-+-{'-' * description_width}-+"
        print("PACMONSTER COMMANDS")
        print(separator)
        print(f"| {'COMMAND'.ljust(command_width)} | {'WHAT IT DOES'.ljust(description_width)} |")
        print(separator)
        for command, description in HELP_COMMANDS:
            print(f"| {command.ljust(command_width)} | {description.ljust(description_width)} |")
        print(separator)


def _dispatch():
    """Dispatch the command selected from the process command line."""

    if len(sys.argv) < 2:
        update_command()
        return

    cmd = sys.argv[1]

    if cmd == "refresh":
        refresh_command()
    elif cmd == "refresh-status":
        refresh_status_command()
    elif cmd == "inventory":
        json_output = "--json" in sys.argv
        inventory_command(json_output)
    elif cmd == "status":
        verbose = "--verbose" in sys.argv
        json_output = "--json" in sys.argv
        status_command(verbose, json_output)
    elif cmd == "doctor":
        json_output = "--json" in sys.argv
        if "--installation" in sys.argv:
            doctor_command(json_output, installation=True)
        else:
            doctor_command(json_output)
    elif cmd == "help":
        help_command(json_output="--json" in sys.argv)
    elif cmd == "dry-run":
        json_output = "--json" in sys.argv
        dry_run_command(json_output)
    elif cmd == "update":
        assume_yes = "--yes" in sys.argv
        update_command(assume_yes, json_output="--json" in sys.argv)
    elif cmd == "start":
        start_command()
    elif cmd == "ghosthunter":
        ghosthunter_command(confirm="--confirm" in sys.argv[2:])
    elif cmd == "witchdoctor":
        witchdoctor_command()
    elif cmd == "advisor":
        _run_tui("advisor")
    elif cmd == "archeologist":
        operation = sys.argv[2] if len(sys.argv) > 2 else None
        if operation is None or operation == "tui":
            _run_tui("archeologist")
        else:
            query = sys.argv[3] if operation == "search" and len(sys.argv) > 3 else None
            archeologist_command(
                operation,
                query,
                json_output="--json" in sys.argv,
            )
    elif cmd == "history":
        json_output = "--json" in sys.argv
        history_command(json_output)
    elif cmd == "librarian":
        operation = sys.argv[2] if len(sys.argv) > 2 else None
        if operation is None or operation == "tui":
            _run_tui("librarian")
        else:
            argument = sys.argv[3] if len(sys.argv) > 3 and not sys.argv[3].startswith("--") else None
            librarian_command(
                operation,
                argument,
                json_output="--json" in sys.argv
            )
    elif cmd == "restorer":
        operation = sys.argv[2] if len(sys.argv) > 2 else None
        if operation is None or operation == "tui":
            _run_tui("restorer")
        else:
            snapshot_id = (
                sys.argv[3]
                if len(sys.argv) > 3 and not sys.argv[3].startswith("--")
                else None
            )
            restorer_command(
                operation,
                snapshot_id,
                assume_yes="--yes" in sys.argv,
                json_output="--json" in sys.argv
            )
    else:
        help_command()


def main():
    """Dispatch a command and exit cleanly when the user presses Ctrl+C."""

    try:
        _dispatch()
    except KeyboardInterrupt:
        print("\nCancellation requested; cleaning up and stopping.", file=sys.stderr, flush=True)
        raise SystemExit(130) from None


if __name__ == "__main__":
    main()
