import subprocess

from cache import save_status_cache
from reboot import host_needs_reboot
from reboot import arch_needs_reboot


CRITICAL_PACKAGES = [
    "linux",
    "linux-lts",
    "linux-zen",
    "systemd",
    "glibc",
    "grub",
    "openssl",
    "sudo"
]


def run(cmd):
    return subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )


def host_updates():

    result = run("apt list --upgradable 2>/dev/null")

    lines = result.stdout.splitlines()

    if len(lines) <= 1:
        return {"count": 0, "critical": False, "packages": []}

    packages = []
    critical = False

    for line in lines[1:]:
        package = line.split("/")[0]
        packages.append(package)

        if package in CRITICAL_PACKAGES:
            critical = True

    return {
        "count": len(packages),
        "critical": critical,
        "packages": packages
    }


def arch_updates(vmid):

    result = run(
        f"pct exec {vmid} -- pacman -Qu"
    )

    if result.returncode == 1:
        return {"count": 0, "critical": False, "packages": []}

    if result.returncode != 0:
        return None

    packages = []
    critical = False

    for line in result.stdout.splitlines():
        package = line.split()[0]
        packages.append(package)

        if package in CRITICAL_PACKAGES:
            critical = True

    return {
        "count": len(packages),
        "critical": critical,
        "packages": packages
    }


def build_status(inventory):

    status = []

    for item in inventory:

        state = "OK"
        updates = 0
        packages = []

        if item.status != "running":
            state = "STOP"
            updates = "-"

        elif item.kind == "host":

            info = host_updates()

            updates = info["count"]
            packages = info["packages"]

            if host_needs_reboot():
                state = "REBOOT"
            elif info["critical"]:
                state = "CRIT"
            elif updates > 0:
                state = "UPD"

        elif item.kind == "lxc":

            result = arch_updates(item.id)

            if result is None:
                state = "ERR"
                updates = "?"

            else:
                updates = result["count"]
                packages = result["packages"]

                if arch_needs_reboot(item.id):
                    state = "REBOOT"
                elif result["critical"]:
                    state = "CRIT"
                elif updates > 0:
                    state = "UPD"

        status.append({
            "id": item.id,
            "name": item.name,
            "state": state,
            "updates": updates,
            "packages": packages
        })

    save_status_cache(status)

    return status
