import subprocess


def run(cmd):
    return subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )


def host_updates():

    result = run(
        "apt list --upgradable 2>/dev/null"
    )

    lines = result.stdout.splitlines()

    if len(lines) <= 1:
        return 0

    return len(lines) - 1


def arch_updates(vmid):

    result = run(
        f"pct exec {vmid} -- pacman -Qu"
    )

    if result.returncode == 1:
        return 0

    if result.returncode != 0:
        return None

    return len(result.stdout.splitlines())
