import sys

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
    inventory = build_inventory()
    add_history("refresh", {"objects": len(inventory)})
    print(f"Inventory cache updated: {len(inventory)} objects")


def refresh_status_command():
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


def inventory_command():
    cache = load_inventory_cache()
    if cache is None:
        print("No inventory cache found.")
        return
    render_inventory(cache["inventory"])


def status_command(verbose=False):

    cache = load_status_cache()

    if cache is None:
        print("No status cache found.")
        return

    if verbose:
        render_status_verbose(cache["status"])
    else:
        render_status(cache["status"])


def doctor_command():
    print()
    print("PACMONSTER DOCTOR")
    print()
    for name, result in run_doctor():
        print(f"[OK] {name}" if result else f"[FAIL] {name}")


def dry_run_command():
    actions = build_dry_run()
    print()
    print("PACMONSTER DRY RUN")
    print()
    if not actions:
        print("No actions required.")
        return
    for action in actions:
        print(action["id"], action["name"], "->", action["command"])


def update_command():
    result = perform_update()

    if result == "LOCKED":
        print("Another update is already running.")
        return

    if result == "ABORTED":
        print("Update aborted.")
        return

    if result is None:
        print("No status cache found.")
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


def history_command():
    print()
    print("PACMONSTER HISTORY")
    print()

    for entry in reversed(load_history()):
        print(entry["timestamp"], entry["type"])

        for key, value in entry.get("details", {}).items():
            print(" ", key + ":", value)

        print()
def help_command():
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
    if len(sys.argv) < 2:
        update_command()
        return

    cmd = sys.argv[1]

    if cmd == "refresh":
        refresh_command()
    elif cmd == "refresh-status":
        refresh_status_command()
    elif cmd == "inventory":
        inventory_command()
    elif cmd == "status":
        verbose = "--verbose" in sys.argv
        status_command(verbose)
    elif cmd == "doctor":
        doctor_command()
    elif cmd == "dry-run":
        dry_run_command()
    elif cmd == "update":
        update_command()
    elif cmd == "history":
        history_command()
    else:
        help_command()


if __name__ == "__main__":
    main()
