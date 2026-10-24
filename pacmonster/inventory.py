from concurrent.futures import ThreadPoolExecutor

from .models import SystemTarget
import socket
from .discovery import discover_lxc
from .discovery import discover_vm
from .discovery import get_host_distro
from .discovery import get_lxc_distro
from .discovery import get_package_manager
from .cache import save_inventory_cache
from .capabilities import detect_unknown_capabilities

MAX_DISCOVERY_WORKERS = 8


def build_lxc_target(lxc):
    """Detect one container's distribution and return its inventory record."""

    try:
        distro = "unknown"
        caps = detect_unknown_capabilities()

        if lxc["status"] == "running":
            distro = get_lxc_distro(lxc["id"])
            package_manager = get_package_manager(distro)

            if package_manager != "unknown":
                caps = {
                    "package_manager": package_manager,
                    "can_check_updates": True,
                    "can_upgrade": True
                }

        return SystemTarget(
            id=lxc["id"],
            name=lxc["name"],
            kind="lxc",
            distro=distro,
            status=lxc["status"],
            package_manager=caps["package_manager"],
            can_check_updates=caps["can_check_updates"],
            can_upgrade=caps["can_upgrade"]
        )

    except Exception:
        return SystemTarget(
            id=lxc["id"],
            name=lxc["name"],
            kind="lxc",
            distro="error",
            status="error"
        )


def build_inventory():
    """Discover hosts, containers, and VMs, then save the inventory cache."""

    inventory = []

    host_distro = get_host_distro()

    inventory.append(
        SystemTarget(
            id="host",
            name=socket.gethostname(),
            kind="host",
            distro=host_distro,
            status="running",
            package_manager=get_package_manager(host_distro),
            can_check_updates=True,
            can_upgrade=True
        )
    )

    lxcs = discover_lxc()
    if lxcs:
        with ThreadPoolExecutor(
            max_workers=min(MAX_DISCOVERY_WORKERS, len(lxcs))
        ) as workers:
            inventory.extend(workers.map(build_lxc_target, lxcs))

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
