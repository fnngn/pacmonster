import sys

from inventory import build_inventory
from cache import load_inventory_cache
from cache import load_status_cache
from status import build_status
from doctor import run_doctor
from update import build_dry_run
from update import perform_update


def render_inventory(data):
    print()
    print("PACMONSTER INVENTORY")
    print()
    for item in data:
        print(
            str(item["id"]).rjust(4),
            str(item["name"]).ljust(12),
            str(item["distro"]).ljust(8),
            item["status"]
        )


def render_status(data):
    print()
    print("PACMONSTER STATUS")
    print()
    for item in data:
        print(
            str(item["id"]).rjust(4),
            str(item["name"]).ljust(12),
            str(item["state"]).ljust(6),
            item["updates"]
        )


def refresh_command():
    inventory = build_inventory()
    print(f"Inventory cache updated: {len(inventory)} objects")


def refresh_status_command():
    inventory = build_inventory()
    status = build_status(inventory)
    print(f"Status cache updated: {len(status)} objects")


def inventory_command():
    cache = load_inventory_cache()
    if cache is None:
        print("No inventory cache found.")
        return
    render_inventory(cache["inventory"])


def status_command():
    cache = load_status_cache()
    if cache is None:
        print("No status cache found.")
        return
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
    for a in actions:
        print(f"{a['id']} {a['name']} -> {a['command']}")


def update_command():

    result = perform_update()

    print()
    print("PACMONSTER UPDATE")
    print()

    if result == "LOCKED":
        print("Another update is already running.")
        return

    if result is None:
        print("No status cache found.")
        return

    if len(result) == 0:
        print("Nothing to update.")
        return

    for item in result:
        print(
          f"WOULD UPDATE: {item['id']} {item['name']}"
        )


def help_command():
    print("refresh")
    print("refresh-status")
    print("inventory")
    print("status")
    print("doctor")
    print("dry-run")
    print("update")


def main():
    if len(sys.argv) < 2:
        help_command()
        return

    cmd = sys.argv[1]

    if cmd == "refresh":
        refresh_command()
    elif cmd == "refresh-status":
        refresh_status_command()
    elif cmd == "inventory":
        inventory_command()
    elif cmd == "status":
        status_command()
    elif cmd == "doctor":
        doctor_command()
    elif cmd == "dry-run":
        dry_run_command()
    elif cmd == "update":
        update_command()
    else:
        help_command()


if __name__ == "__main__":
    main()
