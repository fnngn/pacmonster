from models import SystemTarget
from discovery import discover_lxc
from discovery import discover_vm
from discovery import get_host_distro
from discovery import get_lxc_distro
from cache import save_cache


def build_inventory():

    inventory = []

    inventory.append(
        SystemTarget(
            id="host",
            name="pve",
            kind="host",
            distro=get_host_distro(),
            status="running"
        )
    )

    for lxc in discover_lxc():

        distro = "unknown"

        if lxc["status"] == "running":
            distro = get_lxc_distro(lxc["id"])

        inventory.append(
            SystemTarget(
                id=lxc["id"],
                name=lxc["name"],
                kind="lxc",
                distro=distro,
                status=lxc["status"]
            )
        )

    for vm in discover_vm():

        inventory.append(
            SystemTarget(
                id=vm["id"],
                name=vm["name"],
                kind="vm",
                distro="unknown",
                status=vm["status"]
            )
        )

    save_cache([item.__dict__ for item in inventory])

    return inventory
