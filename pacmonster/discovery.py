import subprocess
import re


def run(cmd):
    result = subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )

    return result.stdout.strip()


def get_host_distro():

    try:
        with open("/etc/os-release") as f:
            content = f.read()

        match = re.search(
            r"^ID=(.*)$",
            content,
            re.MULTILINE
        )

        if match:
            return match.group(1).replace("\"", "")

    except Exception:
        pass

    return "unknown"


def discover_lxc():

    output = run("pct list")

    targets = []

    lines = output.splitlines()

    if len(lines) < 2:
        return targets

    for line in lines[1:]:

        parts = line.split()

        if len(parts) < 3:
            continue

        vmid = parts[0]
        status = parts[1]
        name = parts[-1]

        targets.append(
            {
                "id": vmid,
                "status": status,
                "name": name
            }
        )

    return targets


def discover_vm():

    output = run("qm list")

    targets = []

    lines = output.splitlines()

    if len(lines) < 2:
        return targets

    for line in lines[1:]:

        parts = line.split()

        if len(parts) < 3:
            continue

        vmid = parts[0]
        name = parts[1]
        status = parts[2]

        targets.append(
            {
                "id": vmid,
                "status": status,
                "name": name
            }
        )

    return targets


def get_lxc_distro(vmid):

    cmd = f"pct exec {vmid} -- cat /etc/os-release"

    output = run(cmd)

    match = re.search(
        r"^ID=(.*)$",
        output,
        re.MULTILINE
    )

    if match:
        return match.group(1).replace("\"", "")

    return "unknown"


def get_package_manager(distro):

    if distro in ("debian", "ubuntu", "proxmox"):
        return "apt"

    if distro in ("arch", "archarm"):
        return "pacman"

    if distro in ("fedora",):
        return "dnf"

    return "unknown"
