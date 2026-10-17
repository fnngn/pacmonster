import subprocess


def run(cmd):
    result = subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )

    return result.stdout.strip()


def host_updates():

    output = run("apt list --upgradable 2>/dev/null")

    lines = output.splitlines()

    if len(lines) <= 1:
        return 0

    return len(lines) - 1


def lxc_arch_updates(vmid):

    output = run(
        f"pct exec {vmid} -- checkupdates 2>/dev/null"
    )

    if not output:
        return 0

    return len(output.splitlines())
