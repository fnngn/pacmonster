import sys

from inventory import build_inventory
from cache import load_inventory_cache
from cache import load_status_cache
from status import build_status
from doctor import run_doctor


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

    try:
        inventory = build_inventory()

        print()
        print("Inventory cache updated.")
        print(f"Objects discovered: {len(inventory)}")

    except Exception as exc:
        print()
        print("Refresh failed.")
        print(str(exc))


def refresh_status_command():

    print("Refreshing status...")

    try:
        inventory = build_inventory()
        status = build_status(inventory)

        print()
        print("Status cache updated.")
        print(f"Objects processed: {len(status)}")

    except Exception as exc:
        print()
        print("Status refresh failed.")
        print(str(exc))


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

    checks = run_doctor()

    for name, result in checks:

        if result:
            print(f"[OK]   {name}")
        else:
            print(f"[FAIL] {name}")


def help_command():

    print("pacmonster commands")
    print()
    print("refresh")
    print("refresh-status")
    print("inventory")
    print("status")
    print("doctor")


def main():

    if len(sys.argv) < 2:
        help_command()
        return

    command = sys.argv[1]

    if command == "refresh":
        refresh_command()
        return

    if command == "refresh-status":
        refresh_status_command()
        return

    if command == "inventory":
        inventory_command()
        return

    if command == "status":
        status_command()
        return

    if command == "doctor":
        doctor_command()
        return

    help_command()


if __name__ == "__main__":
    main()
