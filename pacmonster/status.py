import subprocess

from .cache import save_status_cache
from .reboot import host_needs_reboot
from .reboot import arch_needs_reboot


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


def run(command):
    return subprocess.run(
        command,
        text=True,
        capture_output=True
    )


def package_updates(package_manager, target_id=None):
    commands = {
        "apt": ["apt", "list", "--upgradable"],
        "pacman": ["pacman", "-Qu"],
        "dnf": ["dnf", "check-update"],
    }

    command = commands.get(package_manager)
    if command is None:
        return None

    if target_id is not None:
        command = ["pct", "exec", str(target_id), "--", *command]

    result = run(command)
    if package_manager == "pacman" and result.returncode == 1:
        return {"count": 0, "critical": False, "packages": []}
    if package_manager == "dnf" and result.returncode not in (0, 100):
        return None
    if package_manager != "dnf" and result.returncode != 0:
        return None

    lines = result.stdout.splitlines()
    if package_manager == "apt":
        lines = lines[1:]
        packages = [line.split("/", 1)[0] for line in lines if "/" in line]
    elif package_manager == "pacman":
        packages = [line.split()[0] for line in lines if line.split()]
    else:
        packages = [
            line.split()[0].split(".", 1)[0]
            for line in lines
            if len(line.split()) >= 3
            and not line.strip().startswith("Last metadata")
        ]

    return {
        "count": len(packages),
        "critical": any(package in CRITICAL_PACKAGES for package in packages),
        "packages": packages
    }


def build_status(inventory):

    status = []

    for item in inventory:

        state = "OK"
        updates = 0
        packages = []
        critical = False
        needs_reboot = False

        if item.status != "running":
            state = "STOP"
            updates = "-"

        elif item.kind in ("host", "lxc"):

            target_id = item.id if item.kind == "lxc" else None
            info = package_updates(item.package_manager, target_id)

            if info is None:
                state = "ERR"
                updates = "?"
            else:
                updates = info["count"]
                packages = info["packages"]
                critical = info["critical"]
                if item.kind == "host":
                    needs_reboot = host_needs_reboot()
                elif item.package_manager == "pacman":
                    needs_reboot = arch_needs_reboot(item.id)

                if needs_reboot:
                    state = "REBOOT"
                elif critical:
                    state = "CRIT"
                elif updates > 0:
                    state = "UPD"

        status.append({
            "id": item.id,
            "name": item.name,
            "state": state,
            "updates": updates,
            "packages": packages,
            "critical": critical,
            "needs_reboot": needs_reboot
        })

    save_status_cache(status)

    return status
