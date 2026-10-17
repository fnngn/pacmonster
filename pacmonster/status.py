import subprocess

from cache import save_status_cache


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
        return 0, False

    critical = False

    for line in lines:
        if any(p in line.lower() for p in CRITICAL_PACKAGES):
            critical = True

    return len(lines) - 1, critical


def arch_updates(vmid):

    result = run(
        f"pct exec {vmid} -- pacman -Qu"
    )

    if result.returncode == 1:
        return 0, False

    if result.returncode != 0:
        return None

    lines = result.stdout.splitlines()

    critical = False

    for line in lines:
        if any(p in line.lower() for p in CRITICAL_PACKAGES):
            critical = True

    return len(lines), critical


def build_status(inventory):

    status = []

    for item in inventory:

        state = "OK"
        updates = 0

        if item.status != "running":
            state = "STOP"
            updates = "-"

        elif item.kind == "host":

            count, critical = host_updates()

            updates = count

            if critical:
                state = "CRIT"
            elif count > 0:
                state = "UPD"

        elif item.kind == "lxc":

            result = arch_updates(item.id)

            if result is None:
                state = "ERR"
                updates = "?"

            else:
                count, critical = result
                updates = count

                if critical:
                    state = "CRIT"
                elif count > 0:
                    state = "UPD"

        status.append({
            "id": item.id,
            "name": item.name,
            "state": state,
            "updates": updates
        })

    save_status_cache(status)

    return status
