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
        print(json.dumps([
            {
                "name": name,
                "result": result
            }
            for name, result in results
        ], indent=2))
        return

    print()
    print("PACMONSTER DOCTOR")
    print()

    for name, result in results:
        print(f"[OK] {name}" if result else f"[FAIL] {name}")


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
            "duration": duration
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
    else:
        help_command()


if __name__ == "__main__":
    main()
