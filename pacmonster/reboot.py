import subprocess


def run(cmd):
    """Execute a shell check and capture its output and exit status."""

    return subprocess.run(
        cmd,
        shell=True,
        text=True,
        capture_output=True
    )


def host_needs_reboot():
    """Return whether the host has the Debian reboot-required marker."""

    result = run(
        "test -f /var/run/reboot-required"
    )

    return result.returncode == 0


def arch_needs_reboot(vmid):
    """Compare the running and installed Linux kernel versions in a container."""

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
