import sys
import json

from .inventory import build_inventory
from .cache import load_inventory_cache
from .cache import load_status_cache
from .status import build_status
from .doctor import run_doctor
from .update import build_dry_run
from .update import perform_update
from .history import add_history
from .history import load_history
from .librarian import SnapshotError, create_snapshot, list_snapshots, load_snapshot
from .restaurator import RestoreError, render_result, restore_snapshot


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


def doctor_command(json_output=False):
    """Run environment checks and display their results."""

    results = run_doctor()

    if json_output:
        print(json.dumps(results, indent=2))
    else:
        print()
        print("PACMONSTER DOCTOR")
        print()

        for check in results:
            print(
                f"[{check['status'].upper()}] {check['name']}: "
                f"{check['detail']}"
            )

    if any(check["status"] == "fail" for check in results):
        sys.exit(1)


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


def restaurator_command(command, snapshot_id=None, assume_yes=False, json_output=False):
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
        print("Usage: pacmonster restaurator {list|restore SNAPSHOT_ID [--yes]}")
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
        print(action["id"], action["name"], "->", action["command"])


def update_command(assume_yes=False):
    """Run updates and translate scheduler outcomes into CLI exit codes."""

    result = perform_update(assume_yes)

    if result == "LOCKED":
        print("Another update is already running.")
        sys.exit(3)

    if result == "ABORTED":
        print("Update aborted.")
        sys.exit(2)

    if result == "INTERRUPTED":
        print("Update interrupted.")
        sys.exit(130)

    if result == "SNAPSHOT_FAILED":
        sys.exit(4)

    if result is None:
        print("No status cache found.")
        sys.exit(4)

        return

    updated = sum(1 for item in result if item.get("success", False))
    failed = len(result) - updated
    duration = round(sum(item.get("duration", 0) for item in result), 1)

    add_history(
        "update",
        {
            "updated": updated,
            "failed": failed,
            "duration": duration,
            "snapshots": {
                item["id"]: item["snapshot_id"]
                for item in result
            }
        }
    )

    if failed > 0:
        sys.exit(1)

    sys.exit(0)



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
def help_command():
    """Print the supported CLI command names."""

    print("refresh")
    print("refresh-status")
    print("inventory")
    print("status")
    print("status --verbose")
    print("doctor")
    print("dry-run")
    print("update")
    print("history")
    print("librarian list")
    print("librarian snapshot [TARGET_ID]")
    print("librarian show SNAPSHOT_ID")
    print("restaurator list")
    print("restaurator restore SNAPSHOT_ID [--yes]")


def main():
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
        doctor_command(json_output)
    elif cmd == "dry-run":
        json_output = "--json" in sys.argv
        dry_run_command(json_output)
    elif cmd == "update":
        assume_yes = "--yes" in sys.argv
        update_command(assume_yes)
    elif cmd == "history":
        json_output = "--json" in sys.argv
        history_command(json_output)
    elif cmd == "librarian":
        operation = sys.argv[2] if len(sys.argv) > 2 else "list"
        argument = sys.argv[3] if len(sys.argv) > 3 and not sys.argv[3].startswith("--") else None
        librarian_command(
            operation,
            argument,
            json_output="--json" in sys.argv
        )
    elif cmd == "restaurator":
        operation = sys.argv[2] if len(sys.argv) > 2 else "list"
        snapshot_id = sys.argv[3] if len(sys.argv) > 3 and not sys.argv[3].startswith("--") else None
        restaurator_command(
            operation,
            snapshot_id,
            assume_yes="--yes" in sys.argv,
            json_output="--json" in sys.argv
        )
    else:
        help_command()


if __name__ == "__main__":
    main()
