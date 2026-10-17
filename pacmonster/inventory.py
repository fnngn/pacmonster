from .models import SystemTarget
from .discovery import discover_lxc
from .discovery import discover_vm
from .discovery import get_host_distro
from .discovery import get_lxc_distro
from .cache import save_inventory_cache
from .capabilities import detect_arch_capabilities
from .capabilities import detect_unknown_capabilities


def build_inventory():

    inventory = []

    inventory.append(
        SystemTarget(
            id="host",
            name="pve",
            kind="host",
            distro=get_host_distro(),
            status="running",
            package_manager="apt",
            can_check_updates=True,
            can_upgrade=True
        )
    )

    for lxc in discover_lxc():

        try:

            distro = "unknown"
            caps = detect_unknown_capabilities()

            if lxc["status"] == "running":

                distro = get_lxc_distro(lxc["id"])

                if distro == "arch":
                    caps = detect_arch_capabilities(lxc["id"])

            inventory.append(
                SystemTarget(
                    id=lxc["id"],
                    name=lxc["name"],
                    kind="lxc",
                    distro=distro,
                    status=lxc["status"],
                    package_manager=caps["package_manager"],
                    can_check_updates=caps["can_check_updates"],
                    can_upgrade=caps["can_upgrade"]
                )
            )

        except Exception:

            inventory.append(
                SystemTarget(
                    id=lxc["id"],
                    name=lxc["name"],
                    kind="lxc",
                    distro="error",
                    status="error"
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

    save_inventory_cache({
        "inventory": [item.__dict__ for item in inventory]
    })

    return inventory
