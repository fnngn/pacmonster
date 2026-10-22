import os
import tempfile
from types import SimpleNamespace
import unittest
from pathlib import Path
from unittest.mock import patch

from pacmonster.events import EVENT_LOG, STATE_FILE, write_event
from pacmonster.inventory import build_inventory
from pacmonster.models import SystemTarget
from pacmonster.status import build_status, package_updates


class PackageUpdatesTests(unittest.TestCase):
    def test_runtime_files_are_separated_by_effective_user(self):
        uid = os.geteuid()
        self.assertEqual(EVENT_LOG.name, f"pacmonster-events-{uid}.jsonl")
        self.assertEqual(STATE_FILE.name, f"pacmonster-state-{uid}.json")

    def test_event_writer_creates_log_for_configured_runtime_path(self):
        with tempfile.TemporaryDirectory() as directory:
            event_log = Path(directory) / "events.jsonl"
            with patch("pacmonster.events.EVENT_LOG", event_log):
                write_event({"event": "test"})

            self.assertEqual(
                event_log.read_text(),
                '{"event": "test"}\n',
            )

    @patch("pacmonster.status.run")
    def test_pacman_updates_are_parsed(self, run):
        run.return_value = SimpleNamespace(
            returncode=0,
            stdout="linux 6.12.1-1 -> 6.12.2-1\n",
        )

        result = package_updates("pacman")

        self.assertEqual(result, {
            "count": 1,
            "critical": True,
            "packages": ["linux"],
        })
        run.assert_called_once_with(["pacman", "-Qu"])

    @patch("pacmonster.status.run")
    def test_dnf_update_available_exit_code_is_not_an_error(self, run):
        run.return_value = SimpleNamespace(
            returncode=100,
            stdout="openssl.x86_64 3.0.0-1 updates\n",
        )

        result = package_updates("dnf")

        self.assertEqual(result, {
            "count": 1,
            "critical": True,
            "packages": ["openssl"],
        })

    @patch("pacmonster.status.run")
    def test_container_checks_use_pct_exec(self, run):
        run.return_value = SimpleNamespace(returncode=1, stdout="")

        self.assertEqual(
            package_updates("pacman", target_id="101"),
            {"count": 0, "critical": False, "packages": []},
        )
        run.assert_called_once_with(
            ["pct", "exec", "101", "--", "pacman", "-Qu"]
        )

    @patch("pacmonster.status.save_status_cache")
    @patch("pacmonster.status.host_needs_reboot", return_value=False)
    @patch("pacmonster.status.run")
    def test_arch_host_status_uses_pacman(self, run, _needs_reboot, _save_cache):
        run.return_value = SimpleNamespace(
            returncode=0,
            stdout="linux 6.12.1-1 -> 6.12.2-1\n",
        )
        host = SystemTarget(
            id="host",
            name="test-host",
            kind="host",
            distro="arch",
            status="running",
            package_manager="pacman",
        )

        result = build_status([host])

        self.assertEqual(result[0]["state"], "CRIT")
        self.assertEqual(result[0]["updates"], 1)

    @patch("pacmonster.inventory.save_inventory_cache")
    @patch("pacmonster.inventory.discover_vm", return_value=[])
    @patch(
        "pacmonster.inventory.discover_lxc",
        return_value=[{"id": "101", "name": "debian-ct", "status": "running"}],
    )
    @patch("pacmonster.inventory.get_lxc_distro", return_value="debian")
    @patch("pacmonster.inventory.get_host_distro", return_value="arch")
    def test_debian_container_gets_apt_capabilities(
        self, _host_distro, _lxc_distro, _discover_lxc, _discover_vm, _save_cache
    ):
        inventory = build_inventory()

        container = inventory[1]
        self.assertEqual(container.package_manager, "apt")
        self.assertTrue(container.can_check_updates)
        self.assertTrue(container.can_upgrade)


if __name__ == "__main__":
    unittest.main()
