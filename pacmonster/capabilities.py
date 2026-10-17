import subprocess


def run(cmd):
    return subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )


def detect_arch_capabilities(vmid):

    result = run(
        f"pct exec {vmid} -- pacman -Qu"
    )

    can_check = result.returncode in (0, 1)

    return {
        "package_manager": "pacman",
        "can_check_updates": can_check,
        "can_upgrade": can_check
    }


def detect_unknown_capabilities():

    return {
        "package_manager": "unknown",
        "can_check_updates": False,
        "can_upgrade": False
    }
