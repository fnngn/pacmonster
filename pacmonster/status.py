import subprocess


def run(cmd):
    result = subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )

    return result


def host_updates():

    result = run("apt list --upgradable 2>/dev/null")

    lines = result.stdout.splitlines()

    if len(lines) <= 1:
        return 0

    return len(lines) - 1


def lxc_arch_updates(vmid):

    result = run(
        f"pct exec {vmid} -- pacman -Qu"
    )

    if result.returncode == 1:
        return 0

    if result.returncode != 0:
        return -1

    return len(result.stdout.splitlines())
