import os
import json
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from concurrent.futures import ThreadPoolExecutor
from io import StringIO
from pathlib import Path
from threading import Barrier, Lock
from time import monotonic, sleep
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

from pacmonster import archeologist, doctor, librarian, restorer, update
from pacmonster import main as cli_main
from pacmonster.events import EVENT_LOG, STATE_FILE, write_event
from pacmonster.inventory import build_inventory
from pacmonster.models import SystemTarget
from pacmonster.status import build_status, package_updates


class PackageUpdatesTests(unittest.TestCase):
    def test_librarian_cleans_partial_snapshot_when_interrupted(self):
        target = {
            "id": "host",
            "name": "host",
            "kind": "host",
            "package_manager": "pacman",
        }
        with tempfile.TemporaryDirectory() as directory:
            snapshot_root = Path(directory)
            with (
                patch.object(librarian, "SNAPSHOT_ROOT", snapshot_root),
                patch.object(librarian, "package_manifest", return_value={"demo": "1.0"}),
                patch.object(librarian, "_archive_cache", side_effect=KeyboardInterrupt),
                redirect_stdout(StringIO()),
            ):
                with self.assertRaises(KeyboardInterrupt):
                    librarian.create_snapshot(target)

            self.assertEqual(list(snapshot_root.iterdir()), [])

    def test_librarian_removes_temporary_blob_if_storage_is_interrupted(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            source = root / "demo.pkg.tar.zst"
            source.write_bytes(b"package data")
            snapshot_root = root / "snapshots"
            with (
                patch.object(librarian, "SNAPSHOT_ROOT", snapshot_root),
                patch.object(librarian.os, "link", side_effect=KeyboardInterrupt),
            ):
                with self.assertRaises(KeyboardInterrupt):
                    librarian._store_archive_blob(source)

            self.assertEqual(list((snapshot_root / "blobs").iterdir()), [])

    def test_cli_reports_interrupt_without_traceback(self):
        stderr = StringIO()
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "refresh"]),
            patch.object(cli_main, "build_inventory", side_effect=KeyboardInterrupt),
            redirect_stderr(stderr),
        ):
            with self.assertRaises(SystemExit) as exit_error:
                cli_main.main()

        self.assertEqual(exit_error.exception.code, 130)
        self.assertIn("Cancellation requested", stderr.getvalue())

    def test_ghosthunter_launches_tui_update_mode(self):
        with (
            patch.object(cli_main, "_tui_command", return_value=["/usr/bin/pacmonster-tui"]),
            patch.object(
                cli_main.subprocess,
                "run",
                return_value=SimpleNamespace(returncode=0),
            ) as run_tui,
        ):
            cli_main.ghosthunter_command()

        run_tui.assert_called_once_with(
            ["/usr/bin/pacmonster-tui", "--run-ghosthunter"],
            check=False,
        )

    def test_ghosthunter_confirm_launches_update_mode_with_prompts(self):
        with (
            patch.object(cli_main, "_tui_command", return_value=["/usr/bin/pacmonster-tui"]),
            patch.object(
                cli_main.subprocess,
                "run",
                return_value=SimpleNamespace(returncode=0),
            ) as run_tui,
        ):
            cli_main.ghosthunter_command(confirm=True)

        run_tui.assert_called_once_with(
            ["/usr/bin/pacmonster-tui", "--run-updates-confirm"],
            check=False,
        )

    def test_ghosthunter_cli_flag_enables_update_confirmation(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "ghosthunter", "--confirm"]),
            patch.object(cli_main, "ghosthunter_command") as run_ghosthunter,
        ):
            cli_main._dispatch()

        run_ghosthunter.assert_called_once_with(confirm=True)

    def test_ghosthunter_reports_missing_tui(self):
        stderr = StringIO()
        with (
            patch.object(
                cli_main,
                "_tui_command",
                side_effect=FileNotFoundError("TUI unavailable"),
            ),
            redirect_stderr(stderr),
        ):
            with self.assertRaises(SystemExit) as exit_error:
                cli_main.ghosthunter_command()

        self.assertEqual(exit_error.exception.code, 4)
        self.assertIn("TUI unavailable", stderr.getvalue())

    def test_witchdoctor_launches_tui_doctor_mode(self):
        with (
            patch.object(cli_main, "_tui_command", return_value=["/usr/bin/pacmonster-tui"]),
            patch.object(
                cli_main.subprocess,
                "run",
                return_value=SimpleNamespace(returncode=0),
            ) as run_tui,
        ):
            cli_main.witchdoctor_command()

        run_tui.assert_called_once_with(
            ["/usr/bin/pacmonster-tui", "--run-doctor"],
            check=False,
        )

    def test_start_command_launches_main_menu(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "start"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("start")

    def test_advisor_command_launches_reference_tui(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "advisor"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("advisor")

    def test_restorer_without_subcommand_launches_recovery_tui(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "restorer"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("restorer")

    def test_restorer_tui_alias_launches_recovery_tui(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "restorer", "tui"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("restorer")

    def test_restorer_list_subcommand_remains_available_in_cli(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "restorer", "list"]),
            patch.object(cli_main, "restorer_command") as run_restorer,
        ):
            cli_main._dispatch()

        run_restorer.assert_called_once_with(
            "list",
            None,
            assume_yes=False,
            json_output=False,
        )

    def test_help_json_includes_advisor_and_start_commands(self):
        output = StringIO()
        with redirect_stdout(output):
            cli_main.help_command(json_output=True)

        commands = json.loads(output.getvalue())
        self.assertIn("advisor", commands)
        self.assertIn("start", commands)
        self.assertIn("restorer", commands)
        self.assertIn("ghosthunter --confirm", commands)
        self.assertFalse(any("restaurator" in command for command in commands))

    def test_help_text_describes_commands_in_a_table(self):
        output = StringIO()
        with redirect_stdout(output):
            cli_main.help_command()

        help_text = output.getvalue()
        self.assertIn("| COMMAND", help_text)
        self.assertIn("| WHAT IT DOES", help_text)
        self.assertIn("Open the update dashboard and action menu.", help_text)
        self.assertIn("Restore package versions from a snapshot.", help_text)

    def test_archeologist_command_launches_tui(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "archeologist"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("archeologist")

    def test_doctor_installation_check_outputs_structured_results(self):
        output = StringIO()
        checks = [{"name": "TUI", "status": "ok", "detail": "/usr/bin/pacmonster-tui"}]
        with (
            patch.object(cli_main, "run_installation_checks", return_value=checks),
            redirect_stdout(output),
        ):
            cli_main.doctor_command(json_output=True, installation=True)
        self.assertEqual(json.loads(output.getvalue()), checks)

    def test_archeologist_search_and_issue_filter(self):
        with tempfile.TemporaryDirectory() as directory:
            event_log = Path(directory) / "events.jsonl"
            event_log.write_text(
                '{"event":"assigned","target":"host"}\n'
                '{"event":"failed","target":"container"}\n'
            )
            warning_log = Path(directory) / "apt.log"
            warning_log.write_text(
                "W: repository signature warning\n"
                "E: dpkg returned an error\n"
                "installed libgpg-error package\n"
                "All packages configured\n"
            )
            with (
                patch.object(archeologist, "EVENT_LOG", event_log),
                patch.object(archeologist, "LOG_PATHS", (warning_log,)),
            ):
                issues = archeologist.search_logs(issues_only=True)
                searched = archeologist.search_logs(query="container")

        self.assertEqual([item["level"] for item in issues], ["error", "warning", "error"])
        self.assertEqual(
            next(
                item["level"]
                for item in archeologist.search_logs()
                if "libgpg-error" in item["message"]
            ),
            "info",
        )
        self.assertEqual(len(searched), 1)
        self.assertIn("container", searched[0]["message"])

    def test_librarian_without_subcommand_launches_tui(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "librarian"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("librarian")

    def test_librarian_tui_alias_launches_tui(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "librarian", "tui"]),
            patch.object(cli_main, "_run_tui") as run_tui,
        ):
            cli_main._dispatch()

        run_tui.assert_called_once_with("librarian")

    def test_librarian_subcommands_remain_direct_cli_commands(self):
        with (
            patch.object(cli_main.sys, "argv", ["pacmonster", "librarian", "list", "--json"]),
            patch.object(cli_main, "librarian_command") as librarian_cli,
        ):
            cli_main._dispatch()

        librarian_cli.assert_called_once_with("list", None, json_output=True)

    def test_ghosthunter_finds_tui_built_from_its_project_directory(self):
        with tempfile.TemporaryDirectory() as directory:
            tui_root = Path(directory)
            executable = tui_root / "target" / "debug" / "pacmonster-tui"
            executable.parent.mkdir(parents=True)
            (tui_root / "Cargo.toml").touch()
            executable.touch()
            executable.chmod(0o755)
            with (
                patch.object(cli_main.Path, "cwd", return_value=tui_root),
                patch.object(cli_main.shutil, "which", return_value=None),
            ):
                command = cli_main._tui_command()

        self.assertEqual(command, [str(executable)])

    def test_ghosthunter_falls_back_to_cargo_run_from_source_tree(self):
        with tempfile.TemporaryDirectory() as directory:
            project_root = Path(directory)
            (project_root / "tui").mkdir()
            (project_root / "tui" / "Cargo.toml").touch()
            with (
                patch.object(cli_main.Path, "cwd", return_value=project_root),
                patch.object(
                    cli_main,
                    "__file__",
                    str(project_root / "pacmonster" / "main.py"),
                ),
                patch.object(
                    cli_main.shutil,
                    "which",
                    side_effect=lambda command: (
                        "/usr/bin/cargo" if command == "cargo" else None
                    ),
                ),
            ):
                command = cli_main._tui_command()

        self.assertEqual(
            command,
            [
                "/usr/bin/cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(project_root / "tui" / "Cargo.toml"),
                "--",
            ],
        )

    def test_librarian_reports_corrupt_snapshot_metadata(self):
        with tempfile.TemporaryDirectory() as directory:
            snapshot_directory = Path(directory) / "damaged"
            snapshot_directory.mkdir()
            (snapshot_directory / "snapshot.json").write_text("{")

            with patch.object(librarian, "SNAPSHOT_ROOT", Path(directory)):
                with self.assertRaises(librarian.SnapshotError):
                    librarian.list_snapshots()

    def test_librarian_explains_root_requirement_for_system_snapshot_store(self):
        with (
            patch.object(librarian, "SNAPSHOT_ROOT", Path("/var/lib/pacmonster/snapshots")),
            patch.object(librarian.os, "geteuid", return_value=1000),
        ):
            with self.assertRaisesRegex(librarian.SnapshotError, "sudo pacmonster"):
                librarian._require_snapshot_root()

    def test_librarian_deduplicates_identical_package_archives(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            package_archive = root / "demo-1.0-1-x86_64.pkg.tar.zst"
            package_archive.write_bytes(b"native compressed package")
            with patch.object(librarian, "SNAPSHOT_ROOT", root / "snapshots"):
                first = librarian._store_archive_blob(package_archive)
                second = librarian._store_archive_blob(package_archive)

            self.assertEqual(
                {key: value for key, value in first.items() if key != "new"},
                {key: value for key, value in second.items() if key != "new"},
            )
            self.assertTrue(first["new"])
            self.assertFalse(second["new"])
            self.assertEqual(
                len(list((root / "snapshots" / "blobs").iterdir())),
                1,
            )
            self.assertEqual(first["bytes"], package_archive.stat().st_size)

    def test_librarian_stores_only_new_package_versions_across_snapshots(self):
        target = {
            "id": "host",
            "name": "host",
            "kind": "host",
            "distro": "arch",
            "package_manager": "pacman",
        }
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "cache"
            cache.mkdir()
            (cache / "demo-1.0-1-x86_64.pkg.tar.zst").write_bytes(b"demo-v1")
            (cache / "libdemo-1.0-1-x86_64.pkg.tar.zst").write_bytes(b"lib-v1")

            def package_query(command, **_kwargs):
                identities = []
                for value in command[2:]:
                    name = Path(value).name
                    if name.startswith("demo-1.0"):
                        identities.append("demo 1.0-1")
                    elif name.startswith("demo-2.0"):
                        identities.append("demo 2.0-1")
                    elif name.startswith("demo-3.0"):
                        identities.append("demo 3.0-1")
                    elif name.startswith("libdemo-"):
                        identities.append("libdemo 1.0-1")
                return SimpleNamespace(
                    returncode=0,
                    stdout="\n".join(identities),
                    stderr="",
                )

            with (
                patch.object(librarian, "SNAPSHOT_ROOT", root / "snapshots"),
                patch.dict(librarian.CACHE_DIRECTORIES, {"pacman": str(cache)}),
                patch.object(
                    librarian,
                    "package_manifest",
                    side_effect=[
                        {"demo": "1.0-1", "libdemo": "1.0-1"},
                        {"demo": "2.0-1", "libdemo": "1.0-1"},
                        {"demo": "2.0-1", "libdemo": "1.0-1"},
                        {"demo": "3.0-1", "libdemo": "1.0-1"},
                    ],
                ),
                patch.object(
                    librarian.subprocess,
                    "run",
                    side_effect=package_query,
                ) as package_query_run,
                redirect_stdout(StringIO()),
            ):
                first = librarian.create_snapshot(target)
                (cache / "demo-2.0-1-x86_64.pkg.tar.zst").write_bytes(b"demo-v2")
                second = librarian.create_snapshot(target)
                third = librarian.create_snapshot(target)
                (cache / "demo-3.0-1-x86_64.pkg.tar.zst").write_bytes(b"demo-v3")
                fourth = librarian.create_snapshot(target)
                blob_directory = root / "snapshots" / "blobs"

            self.assertEqual(first["new_archive_bytes"], first["archive_bytes"])
            self.assertEqual(
                second["new_archive_bytes"],
                second["archives"]["demo"]["bytes"],
            )
            self.assertEqual(third["new_archive_bytes"], 0)
            self.assertEqual(fourth["new_archive_bytes"], fourth["archives"]["demo"]["bytes"])
            self.assertEqual(len(list(blob_directory.iterdir())), 3)
            self.assertFalse((root / "snapshots" / first["id"]).exists())
            self.assertEqual(package_query_run.call_count, 3)

    def test_librarian_reuses_identical_archive_blob_from_another_lxc(self):
        digest = "a" * 64
        archive = {
            "blob": f"{digest}.pkg.tar.zst",
            "sha256": digest,
            "bytes": 1024,
            "new": False,
        }
        target = {"id": "102", "kind": "lxc"}
        with patch.object(
            librarian.subprocess,
            "run",
            return_value=SimpleNamespace(
                returncode=0,
                stdout=f"{digest}  /var/cache/pacman/pkg/demo-1.0.pkg.tar.zst\n",
                stderr="",
            ),
        ) as run_command:
            reused = librarian._reuse_remote_blob(
                target,
                "/var/cache/pacman/pkg/demo-1.0.pkg.tar.zst",
                "demo",
                "1.0",
                {("demo", "1.0"): {digest: archive}},
            )

        self.assertEqual(reused, archive)
        self.assertEqual(
            run_command.call_args.args[0],
            [
                "pct", "exec", "102", "--",
                "sha256sum", "--", "/var/cache/pacman/pkg/demo-1.0.pkg.tar.zst",
            ],
        )

    def test_librarian_does_not_reuse_different_lxc_archive_content(self):
        target = {"id": "102", "kind": "lxc"}
        with patch.object(
            librarian.subprocess,
            "run",
            return_value=SimpleNamespace(
                returncode=0,
                stdout=f"{'b' * 64}  /tmp/demo.pkg.tar.zst\n",
                stderr="",
            ),
        ):
            reused = librarian._reuse_remote_blob(
                target,
                "/tmp/demo.pkg.tar.zst",
                "demo",
                "1.0",
                {
                    ("demo", "1.0"): {
                        "a" * 64: {"blob": "a" * 64 + ".pkg.tar.zst"}
                    }
                },
            )

        self.assertIsNone(reused)

    def test_librarian_keeps_only_three_snapshots_per_target(self):
        target = {
            "id": "host",
            "name": "host",
            "kind": "host",
            "distro": "arch",
            "package_manager": "pacman",
        }
        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.object(librarian, "SNAPSHOT_ROOT", Path(directory)),
                patch.object(librarian, "package_manifest", return_value={"demo": "1.0"}),
                patch.object(librarian, "_archive_cache") as archive_cache,
            ):
                archive_cache.return_value = {
                    "demo": {
                        "blob": "a" * 64 + ".pkg.tar.zst",
                        "sha256": "a" * 64,
                        "bytes": 123,
                        "new": True,
                    }
                }
                snapshots = [librarian.create_snapshot(target) for _ in range(4)]

                self.assertEqual(len(librarian.list_snapshots("host")), 3)
                self.assertEqual(snapshots[-1]["archive_bytes"], 123)
                self.assertFalse(
                    (Path(directory) / snapshots[0]["id"]).exists()
                )
                archive_cache.assert_called_with(target, {"demo": "1.0"})

    def test_librarian_keeps_only_cache_archives_matching_installed_versions(self):
        with tempfile.TemporaryDirectory() as directory:
            archives = [
                Path(directory) / "demo-1.0-1-x86_64.pkg.tar.zst",
                Path(directory) / "demo-2.0-1-x86_64.pkg.tar.zst",
            ]
            with patch.object(
                librarian.subprocess,
                "run",
                return_value=SimpleNamespace(
                    returncode=0,
                    stdout="demo 1.0-1\ndemo 2.0-1\n",
                    stderr="",
                ),
            ) as query:
                matching = librarian._matching_cache_archives(
                    "pacman",
                    archives,
                    {"demo": "1.0-1"},
                )

        self.assertEqual(matching, [archives[0]])
        self.assertEqual(len(query.call_args.args[0]), 4)

    def test_librarian_reads_deb_package_identity_and_architecture(self):
        with patch.object(
            librarian.subprocess,
            "run",
            return_value=SimpleNamespace(
                returncode=0,
                stdout="libdemo\n1.0\namd64\n",
                stderr="",
            ),
        ):
            identity = librarian._archive_identity("apt", Path("libdemo_1.0_amd64.deb"))

        self.assertEqual(identity, ("libdemo", "1.0", "amd64"))

    def test_doctor_reports_structured_prerequisite_and_snapshot_checks(self):
        with tempfile.TemporaryDirectory() as directory:
            with (
                patch.object(doctor.os, "geteuid", return_value=0),
                patch.object(doctor, "SNAPSHOT_ROOT", Path(directory)),
                patch.object(doctor, "get_host_distro", return_value="arch"),
                patch.object(doctor, "get_package_manager", return_value="pacman"),
                patch.object(doctor, "command_exists", side_effect=lambda name: name == "pacman"),
                patch.object(doctor, "load_inventory_cache", return_value=None),
                patch.object(doctor, "load_status_cache", return_value=None),
                patch.object(doctor, "list_snapshots", return_value=[]),
                patch.object(
                    doctor.subprocess,
                    "run",
                    return_value=SimpleNamespace(returncode=0, stdout="", stderr=""),
                ) as health_check,
                patch.object(doctor.shutil, "disk_usage", return_value=SimpleNamespace(free=3 * 1024**3)),
            ):
                checks = doctor.run_doctor()

        checks_by_name = {check["name"]: check for check in checks}
        self.assertEqual(checks_by_name["host package manager"]["status"], "ok")
        self.assertEqual(checks_by_name["package database health"]["status"], "ok")
        self.assertEqual(checks_by_name["pct command"]["status"], "skip")
        self.assertEqual(checks_by_name["latest package snapshot"]["status"], "warning")
        self.assertIn("detail", checks_by_name["snapshot disk space"])
        health_check.assert_any_call(
            ["pacman", "-Dk"],
            capture_output=True,
            text=True,
            check=False,
            timeout=60,
        )

    def test_restorer_installs_only_archives_matching_snapshot_version(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            snapshot_id = "host-host-test"
            snapshot_directory = root / snapshot_id
            cache_directory = root / "blobs"
            cache_directory.mkdir(parents=True)
            source_archive = root / "demo-1.0-1-x86_64.pkg.tar.zst"
            source_archive.write_text("test package archive")
            digest = librarian._sha256(source_archive)
            package_archive = cache_directory / (digest + ".pkg.tar.zst")
            package_archive.write_bytes(source_archive.read_bytes())
            (snapshot_directory).mkdir()

            metadata = {
                "id": snapshot_id,
                "target": {
                    "id": "host",
                    "name": "test-host",
                    "kind": "host",
                    "package_manager": "pacman",
                },
                "packages": {"demo": "1.0"},
                "archives": {
                    "demo": {
                        "blob": package_archive.name,
                        "sha256": digest,
                        "bytes": package_archive.stat().st_size,
                    }
                },
            }
            (snapshot_directory / "snapshot.json").write_text(json.dumps(metadata))
            process_result = SimpleNamespace(
                returncode=0,
                stdout="restored",
                stderr="",
            )
            with (
                patch.object(librarian, "SNAPSHOT_ROOT", root),
                patch.object(
                    librarian,
                    "package_manifest",
                    return_value={"demo": "2.0"},
                ),
                patch.object(
                    restorer,
                    "_archive_package",
                    return_value=("demo", "1.0"),
                ),
                patch.object(
                    restorer.subprocess,
                    "run",
                    return_value=process_result,
                ) as run_command,
                patch.object(restorer, "acquire_lock", return_value=True),
                patch.object(restorer, "release_lock"),
                redirect_stdout(StringIO()),
            ):
                result = restorer.restore_snapshot(snapshot_id, assume_yes=True)

        self.assertEqual(result["status"], "success")
        self.assertIn(package_archive.name, result["restored"])
        self.assertEqual(run_command.call_args.args[0][:3], ["pacman", "-U", "--noconfirm"])

    def test_restorer_will_not_compete_with_an_existing_update(self):
        with (
            patch.object(restorer, "acquire_lock", return_value=False),
            patch.object(librarian, "load_snapshot") as load_snapshot,
        ):
            result = restorer.restore_snapshot("snapshot-id")

        self.assertEqual(result["status"], "locked")
        load_snapshot.assert_not_called()

    def test_restorer_matches_multiarch_deb_and_uses_local_dnf_archives(self):
        with tempfile.TemporaryDirectory() as directory:
            cache_directory = Path(directory) / "package-cache"
            cache_directory.mkdir()
            package_archive = cache_directory / "libdemo_1.0_amd64.deb"
            package_archive.touch()
            snapshot = {
                "target": {"package_manager": "apt"},
                "packages": {"libdemo:amd64": "1.0"},
            }
            with patch.object(
                restorer,
                "_archive_package",
                return_value=("libdemo:amd64", "1.0"),
            ):
                archives, missing = restorer._find_restore_packages(
                    snapshot,
                    Path(directory),
                    {"libdemo:amd64": "2.0"},
                )

        self.assertEqual(archives, [package_archive])
        self.assertEqual(missing, [])
        self.assertEqual(
            restorer._restore_command(
                {"target": {"package_manager": "dnf"}},
                [Path("/tmp/demo-1.0.rpm")],
            ),
            ["dnf", "install", "-y", "/tmp/demo-1.0.rpm"],
        )

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
            patch.object(
                update,
                "create_snapshot",
                side_effect=lambda action: {
                    "id": f"snapshot-{action['id']}",
                    "package_count": 1,
                    "restorable": True,
                },
            ),
            patch.object(update, "execute_action", side_effect=run_action),
            patch.object(update, "emit_ghost_event"),
            patch.object(update, "emit_pool_event"),
            patch.object(update, "get_worker_count", return_value=2),
            redirect_stdout(StringIO()),
        ):
            results = update.perform_update(assume_yes=True)

        self.assertEqual(len(results), len(actions))
        self.assertTrue(all(result["success"] for result in results))
        self.assertEqual(maximum_active, 2)

    def test_update_worker_count_respects_cpu_memory_load_and_safety_cap(self):
        with (
            patch.object(update, "effective_cpu_cores", return_value=16),
            patch.object(
                update,
                "available_memory_bytes",
                return_value=64 * 1024**3,
            ),
            patch.object(update.os, "getloadavg", return_value=(0.0, 0.0, 0.0)),
        ):
            self.assertEqual(update.get_worker_count(20), 8)

        with (
            patch.object(update, "effective_cpu_cores", return_value=4),
            patch.object(
                update,
                "available_memory_bytes",
                return_value=64 * 1024**3,
            ),
            patch.object(update.os, "getloadavg", return_value=(0.0, 0.0, 0.0)),
        ):
            self.assertEqual(update.get_worker_count(10), 2)

        with (
            patch.object(update, "effective_cpu_cores", return_value=16),
            patch.object(
                update,
                "available_memory_bytes",
                return_value=1024**3,
            ),
            patch.object(update.os, "getloadavg", return_value=(0.0, 0.0, 0.0)),
        ):
            self.assertEqual(update.get_worker_count(10), 1)

        with (
            patch.object(update, "effective_cpu_cores", return_value=8),
            patch.object(
                update,
                "available_memory_bytes",
                return_value=64 * 1024**3,
            ),
            patch.object(update.os, "getloadavg", return_value=(7.0, 0.0, 0.0)),
        ):
            self.assertEqual(update.get_worker_count(10), 1)

    def test_ghost_progress_is_capped_and_reset_on_release(self):
        pool = update.GhostPool(max_ghosts=1, cpu_cores=2)
        ghost = pool.allocate("host", estimated_seconds=10)

        pool.update_progress(ghost.id, 7)
        snapshot = pool.snapshot()
        self.assertEqual(snapshot["cpu_cores"], 2)
        self.assertEqual(snapshot["ghosts"][0]["progress"], 70)

        pool.update_progress(ghost.id, 20)
        self.assertEqual(pool.snapshot()["ghosts"][0]["progress"], 95)

        pool.set_limit(1)
        self.assertEqual(pool.snapshot()["capacity"], 1)

        pool.release(ghost.id)
        self.assertIsNone(pool.snapshot()["ghosts"][0]["progress"])

    def test_progress_estimate_uses_recent_successful_target_runs(self):
        history = [
            {
                "type": "update",
                "details": {
                    "targets": [
                        {"id": "host", "duration": 30, "success": True},
                        {"id": "host", "duration": 90, "success": False},
                    ]
                },
            },
            {
                "type": "update",
                "details": {
                    "targets": [{"id": "host", "duration": 50, "success": True}]
                },
            },
        ]
        with patch("pacmonster.history.load_history", return_value=history):
            estimate = update.estimate_action_seconds({"id": "host"})

        self.assertEqual(estimate, 40)

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
            patch.object(
                update,
                "create_snapshot",
                return_value={
                    "id": "snapshot-1",
                    "package_count": 1,
                    "restorable": True,
                },
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

    def test_update_does_not_start_if_snapshot_creation_fails(self):
        with (
            patch.object(update, "acquire_lock", return_value=True),
            patch.object(update, "release_lock") as release_lock,
            patch.object(
                update,
                "build_dry_run",
                return_value=[{"id": "1", "name": "host"}],
            ),
            patch.object(
                update,
                "create_snapshot",
                side_effect=librarian.SnapshotError("disk unavailable"),
            ),
            patch.object(update, "execute_action") as execute_action,
            redirect_stdout(StringIO()),
        ):
            result = update.perform_update(assume_yes=True)

        self.assertEqual(result, "SNAPSHOT_FAILED")
        execute_action.assert_not_called()
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
