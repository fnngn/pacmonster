import subprocess


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

    result = run(
        "apt list --upgradable 2>/dev/null"
    )

    lines = result.stdout.splitlines()

    if len(lines) <= 1:
        return (0, False)

    critical = False

    for line in lines:
        lower = line.lower()

        if any(pkg in lower for pkg in CRITICAL_PACKAGES):
            critical = True

    return (len(lines) - 1, critical)


def arch_updates(vmid):

    result = run(
        f"pct exec {vmid} -- pacman -Qu"
    )

    if result.returncode == 1:
        return (0, False)

    if result.returncode != 0:
        return None

    lines = result.stdout.splitlines()

    critical = False

    for line in lines:
        lower = line.lower()

        if any(pkg in lower for pkg in CRITICAL_PACKAGES):
            critical = True

    return (len(lines), critical)
