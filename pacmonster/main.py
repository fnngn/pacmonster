import sys

from inventory import build_inventory
from cache import load_inventory_cache
from cache import load_status_cache
from status import build_status
from doctor import run_doctor
from update import build_dry_run


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
    print("Refreshing inventory...")
    inventory = build_inventory()
    print()
    print("Inventory cache updated.")
    print(f"Objects discovered: {len(inventory)}")


def refresh_status_command():
    print("Refreshing status...")
    inventory = build_inventory()
    status = build_status(inventory)
    print()
    print("Status cache updated.")
    print(f"Objects processed: {len(status)}")


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
        if result:
            print(f"[OK]   {name}")
        else:
            print(f"[FAIL] {name}")


def dry_run_command():

    actions = build_dry_run()

    if actions is None:
        print("No status cache found.")
        print("Run: refresh-status")
        return

    print()
    print("PACMONSTER DRY RUN")
    print()

    if not actions:
        print("No actions required.")
        return

    for item in actions:
        print(
            f"{item['id']} {item['name']} -> {item['command']}"
        )

    print()
    print(f"Would update: {len(actions)}")


def help_command():
    print("pacmonster commands")
    print()
    print("refresh")
    print("refresh-status")
    print("inventory")
    print("status")
    print("doctor")
    print("dry-run")


def main():
    if len(sys.argv) < 2:
        help_command()
        return

    command = sys.argv[1]

    if command == "refresh":
        refresh_command()
    elif command == "refresh-status":
        refresh_status_command()
    elif command == "inventory":
        inventory_command()
    elif command == "status":
        status_command()
    elif command == "doctor":
        doctor_command()
    elif command == "dry-run":
        dry_run_command()
    else:
        help_command()


if __name__ == "__main__":
    main()
