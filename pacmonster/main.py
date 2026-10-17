import sys

from inventory import build_inventory
from cache import load_cache
from status import host_updates
from status import arch_updates


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

    try:
        inventory = build_inventory()

        print()
        print("Inventory cache updated.")
        print(f"Objects discovered: {len(inventory)}")

    except Exception as exc:
        print()
        print("Refresh failed.")
        print(str(exc))


def inventory_command():

    cache = load_cache()

    if cache is None:
        print("No cache found. Run: pacmonster refresh")
        return

    render_inventory(cache["inventory"])


def status_command():

    inventory = build_inventory()

    print()
    print("PACMONSTER STATUS")
    print("=" * 90)

    print(
        "ID".rjust(6),
        "NAME".ljust(18),
        "STATE".ljust(12),
        "UPDATES".rjust(8)
    )

    print("-" * 90)

    for item in inventory:

        state = "OK"
        updates = "-"

        if item.status != "running":
            state = "STOPPED"

        elif item.kind == "host":

            count, critical = host_updates()
            updates = str(count)

            if critical:
                state = "CRITICAL"
            elif count > 0:
                state = "UPDATES"

        elif item.kind == "lxc":

            if item.distro == "arch":

                result = arch_updates(item.id)

                if result is None:
                    state = "ERR"
                    updates = "?"

                else:

                    count, critical = result

                    updates = str(count)

                    if critical:
                        state = "CRITICAL"
                    elif count > 0:
                        state = "UPDATES"

        print(
            str(item.id).rjust(6),
            str(item.name).ljust(18),
            state.ljust(12),
            str(updates).rjust(8)
        )


def help_command():

    print("pacmonster commands")
    print()
    print("refresh")
    print("inventory")
    print("status")


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

    if command == "status":
        status_command()
        return

    help_command()


if __name__ == "__main__":
    main()
