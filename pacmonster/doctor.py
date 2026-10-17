import subprocess

from cache import load_inventory_cache
from cache import load_status_cache


def command_exists(cmd):

    result = subprocess.run(
        f"command -v {cmd}",
        shell=True,
        capture_output=True
    )

    return result.returncode == 0


def run_doctor():

    checks = []

    checks.append((
        "inventory cache",
        load_inventory_cache() is not None
    ))

    checks.append((
        "status cache",
        load_status_cache() is not None
    ))

    checks.append((
        "pct available",
        command_exists("pct")
    ))

    checks.append((
        "qm available",
        command_exists("qm")
    ))

    return checks
