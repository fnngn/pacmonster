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


def inventory_command():
    inventory = build_inventory()
    render_inventory([i.__dict__ for i in inventory])


def help_command():
    print("pacmonster commands")
    print()
    print("inventory")


def main():
    if len(sys.argv) < 2:
        help_command()
        return

    if sys.argv[1] == "inventory":
        inventory_command()
        return

    help_command()


if __name__ == "__main__":
    main()
