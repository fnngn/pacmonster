import subprocess


def run(cmd):
    return subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )


def host_needs_reboot():

    result = run(
        "test -f /var/run/reboot-required"
    )

    return result.returncode == 0


def arch_needs_reboot(vmid):

    running = run(
        f"pct exec {vmid} -- uname -r"
    )

    installed = run(
        f"pct exec {vmid} -- pacman -Q linux"
    )

    if running.returncode != 0:
        return False

    if installed.returncode != 0:
        return False

    running_kernel = running.stdout.strip()

    installed_kernel = installed.stdout.split()[-1]

    return running_kernel not in installed_kernel
