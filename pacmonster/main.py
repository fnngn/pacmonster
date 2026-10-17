import sys

from inventory import build_inventory
from cache import load_cache


def render_inventory(data):

    print()
    print("PACMONSTER INVENTORY")
    print("=" * 110)

    print(
        "ID".rjust(6),
        "TYPE".ljust(8),
        "NAME".ljust(16),
        "DISTRO".ljust(10),
        "PKG".ljust(10),
        "CHECK".ljust(8),
        "UPGRADE".ljust(8),
        "STATUS"
    )

    print("-" * 110)

    for item in data:
        print(
            str(item["id"]).rjust(6),
            str(item["kind"]).ljust(8),
            str(item["name"]).ljust(16),
            str(item["distro"]).ljust(10),
            str(item["package_manager"]).ljust(10),
            str(item["can_check_updates"]).ljust(8),
            str(item["can_upgrade"]).ljust(8),
            item["status"]
        )


def refresh_command():

    print("Refreshing inventory...")

    build_inventory()

    print("Inventory cache updated.")


def inventory_command():

    cache = load_cache()

    if cache is None:
        print("No cache found. Run: pacmonster refresh")
        return

    render_inventory(cache["inventory"])


def help_command():

    print("pacmonster commands")
    print()
    print("refresh      rebuild cache")
    print("inventory    show cached inventory")
    print("status       coming soon")
    print("update       coming soon")


def main():

    if len(sys.argv) < 2:
        help_command()
        return

    command = sys.argv[1]

    if command == "refresh":
        refresh_command()
        return

    if command == "inventory":
        inventory_command()
        return

    help_command()


if __name__ == "__main__":
    main()
