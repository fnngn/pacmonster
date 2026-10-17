import subprocess


def run(cmd):
    result = subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )

    return result.returncode


def detect_arch_capabilities(vmid):

    pacman = run(
        f"pct exec {vmid} -- which pacman"
    ) == 0

    checkupdates = run(
        f"pct exec {vmid} -- which checkupdates"
    ) == 0

    return {
        "package_manager": "pacman",
        "can_check_updates": checkupdates,
        "can_upgrade": pacman
    }


def detect_unknown_capabilities():

    return {
        "package_manager": "unknown",
        "can_check_updates": False,
        "can_upgrade": False
    }
