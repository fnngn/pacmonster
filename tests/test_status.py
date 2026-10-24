import os
import tempfile
from contextlib import redirect_stdout
from concurrent.futures import ThreadPoolExecutor
from io import StringIO
from pathlib import Path
from threading import Barrier, Lock
from time import monotonic, sleep
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from pacmonster import update
from pacmonster.events import EVENT_LOG, STATE_FILE, write_event
from pacmonster.inventory import build_inventory
from pacmonster.models import SystemTarget
from pacmonster.status import build_status, package_updates


class PackageUpdatesTests(unittest.TestCase):
    def test_package_command_output_is_not_captured(self):
        process = Mock()
        process.wait.return_value = 0
        action = {"name": "test-container", "command": "printf progress"}

        with patch.object(update.subprocess, "Popen", return_value=process) as popen:
            result = update.ActionRunner().execute(action)

        self.assertTrue(result["success"])
        self.assertEqual(result["stdout"], "")
        self.assertEqual(result["stderr"], "")
        popen.assert_called_once_with(
            action["command"],
            shell=True,
            text=True,
            start_new_session=True,
        )

    def test_interrupt_stops_package_command_process_group(self):
        runner = update.ActionRunner()
        with ThreadPoolExecutor(max_workers=1) as executor:
            future = executor.submit(
                runner.execute,
                {"command": "sleep 30"},
            )

            deadline = monotonic() + 2
            while monotonic() < deadline:
                with runner._lock:
                    if runner._processes:
                        break
                sleep(0.01)
            else:
                self.fail("package command did not start")

            runner.stop()
            result = future.result(timeout=3)

        self.assertFalse(result["success"])

    def test_update_scheduler_refills_ghosts_until_queue_is_empty(self):
        actions = [
            {"id": str(index), "name": f"container-{index}"}
            for index in range(5)
        ]
        first_pair_started = Barrier(2, timeout=2)
        active_lock = Lock()
        active = 0
        maximum_active = 0

        def run_action(action, _runner):
            nonlocal active, maximum_active
            with active_lock:
                active += 1
                maximum_active = max(maximum_active, active)

            if action["id"] in ("0", "1"):
                first_pair_started.wait()
            sleep(0.01)

            with active_lock:
                active -= 1

            return {
                "action": action,
                "stdout": "",
                "stderr": "",
                "returncode": 0,
                "success": True,
                "duration": 0.01,
            }

        with (
            patch.object(update, "acquire_lock", return_value=True),
            patch.object(update, "release_lock"),
            patch.object(update, "build_dry_run", return_value=actions),
            patch.object(update, "execute_action", side_effect=run_action),
            patch.object(update, "emit_ghost_event"),
            patch.object(update, "emit_pool_event"),
            patch.object(update, "GHOSTS", 2),
            redirect_stdout(StringIO()),
        ):
            results = update.perform_update(assume_yes=True)

        self.assertEqual(len(results), len(actions))
        self.assertTrue(all(result["success"] for result in results))
        self.assertEqual(maximum_active, 2)

    def test_update_interrupt_stops_runner_and_releases_lock(self):
        runner = Mock()
        with (
            patch.object(update, "acquire_lock", return_value=True),
            patch.object(update, "release_lock") as release_lock,
            patch.object(
                update,
                "build_dry_run",
                return_value=[{"id": "1", "name": "container-1"}],
            ),
            patch.object(update, "ActionRunner", return_value=runner),
            patch.object(update, "wait", side_effect=KeyboardInterrupt),
            patch.object(update, "execute_action"),
            patch.object(update, "emit_ghost_event"),
            patch.object(update, "emit_pool_event"),
            redirect_stdout(StringIO()),
        ):
            result = update.perform_update(assume_yes=True)

        self.assertEqual(result, "INTERRUPTED")
        runner.stop.assert_called_once_with()
        release_lock.assert_called_once_with()

    @patch("pacmonster.inventory.save_inventory_cache")
    @patch("pacmonster.inventory.discover_vm", return_value=[])
    @patch(
        "pacmonster.inventory.discover_lxc",
        return_value=[
            {"id": "101", "name": "container-a", "status": "running"},
            {"id": "102", "name": "container-b", "status": "running"},
        ],
    )
    @patch("pacmonster.inventory.get_lxc_distro")
    @patch("pacmonster.inventory.get_host_distro", return_value="arch")
    def test_container_discovery_runs_concurrently(
        self, _host_distro, get_lxc_distro, _discover_lxc, _discover_vm, _save_cache
    ):
        both_started = Barrier(2, timeout=2)

        def detect_distro(_container_id):
            both_started.wait()
            return "debian"

        get_lxc_distro.side_effect = detect_distro

        inventory = build_inventory()

        self.assertEqual([item.id for item in inventory[1:]], ["101", "102"])
        self.assertEqual(
            [item.package_manager for item in inventory[1:]],
            ["apt", "apt"],
        )

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
