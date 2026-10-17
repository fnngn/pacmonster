import sys

from inventory import build_inventory
from cache import load_cache
from status import host_updates
from status import lxc_arch_updates


def render_inventory(data):

    print()
    print("PACMONSTER INVENTORY")
    print("=" * 70)

    print(
        "ID".rjust(6),
        "TYPE".ljust(8),
        "NAME".ljust(16),
        "DISTRO".ljust(12),
        "STATUS"
    )

    print("-" * 70)

    for item in data:
        print(
            str(item["id"]).rjust(6),
            str(item["kind"]).ljust(8),
            str(item["name"]).ljust(16),
            str(item["distro"]).ljust(12),
            item["status"]
        )


def inventory_command():

    inventory = build_inventory()

    render_inventory(
        [i.__dict__ for i in inventory]
    )


def fast_inventory_command():

    cache = load_cache()

    if cache is None:
        inventory_command()
        return

    render_inventory(cache["inventory"])


def status_command():

    inventory = build_inventory()

    print()
    print("PACMONSTER STATUS")
    print("=" * 70)

    print(
        "ID".rjust(6),
        "TYPE".ljust(8),
        "NAME".ljust(16),
        "UPDATES".rjust(8)
    )

    print("-" * 70)

    for item in inventory:

        updates = 0

        if item.kind == "host":
            updates = host_updates()

        elif item.kind == "lxc":

            if item.status == "running":
                if item.distro == "arch":
                    updates = lxc_arch_updates(item.id)

        print(
            str(item.id).rjust(6),
            str(item.kind).ljust(8),
            str(item.name).ljust(16),
            str(updates).rjust(8)
        )


def help_command():

    print("pacmonster commands")
    print()
    print("inventory          refresh inventory")
    print("inventory-fast     use cache")
    print("status             update overview")
    print("update             coming soon")


def main():

    if len(sys.argv) < 2:
        help_command()
        return

    command = sys.argv[1]

    if command == "inventory":
        inventory_command()
        return

    if command == "inventory-fast":
        fast_inventory_command()
        return

    if command == "status":
        status_command()
        return

    help_command()


if __name__ == "__main__":
    main()
