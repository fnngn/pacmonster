"""Plan and execute package restoration from Librarian snapshots."""

from pathlib import Path
import re
import shutil
import subprocess
import sys
import tarfile
import tempfile
from uuid import uuid4

from . import librarian
from .lock import acquire_lock, release_lock


class RestoreError(RuntimeError):
    """Raised when a package snapshot cannot be safely restored."""


def _archive_package(package_manager, archive_path):
    """Read a package name and version from one native package archive."""

    try:
        if package_manager == "apt":
            command = [
                "dpkg-deb", "-f", str(archive_path),
                "Package", "Version", "Architecture",
            ]
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            fields = result.stdout.splitlines()
            if result.returncode == 0 and len(fields) >= 2:
                package = fields[0]
                if len(fields) >= 3 and fields[2]:
                    package = f"{package}:{fields[2]}"
                return package, fields[1]
        elif package_manager == "pacman":
            command = ["pacman", "-Qp", str(archive_path)]
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            fields = result.stdout.split()
            if result.returncode == 0 and len(fields) >= 2:
                return fields[0], fields[1]
        elif package_manager == "dnf":
            command = [
                "rpm", "-qp", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}.%{ARCH}\n",
                str(archive_path),
            ]
            result = subprocess.run(command, capture_output=True, text=True, check=False)
            fields = result.stdout.strip().split("\t", 1)
            if result.returncode == 0 and len(fields) == 2:
                return fields[0], fields[1]
    except OSError as error:
        raise RestoreError(f"Cannot inspect package archive {archive_path}: {error}") from error
    return None


def _extract_cache(snapshot):
    """Extract a snapshot cache after rejecting unsafe archive member paths."""

    if "archives" in snapshot:
        return None
    archive_name = snapshot.get("archive")
    if not archive_name:
        return None

    archive_path = Path(snapshot["path"]) / archive_name
    if archive_path.is_symlink() or not archive_path.is_file():
        raise RestoreError(f"Snapshot archive is missing: {archive_path}")
    expected_digest = snapshot.get("archive_sha256")
    if expected_digest:
        try:
            if librarian._sha256(archive_path) != expected_digest:
                raise RestoreError("Snapshot archive checksum does not match its metadata")
        except OSError as error:
            raise RestoreError(f"Cannot read snapshot archive: {error}") from error
    extraction_path = Path(tempfile.mkdtemp(prefix="pacmonster-restore-"))
    try:
        with tarfile.open(archive_path, "r:*") as archive:
            members = archive.getmembers()
            for member in members:
                member_path = Path(member.name)
                if member_path.is_absolute() or ".." in member_path.parts:
                    raise RestoreError("Snapshot archive contains an unsafe path")
                if not member.isdir() and not member.isfile():
                    raise RestoreError("Snapshot archive contains a non-file entry")
            archive.extractall(extraction_path, members=members)
    except Exception as error:
        shutil.rmtree(extraction_path, ignore_errors=True)
        if isinstance(error, RestoreError):
            raise
        raise RestoreError(f"Cannot extract snapshot package cache: {error}") from error

    return extraction_path


def _find_restore_packages(snapshot, extraction_path, current_packages):
    """Find exact-version cached archives for packages differing from snapshot."""

    desired = snapshot["packages"]
    changed = {
        name: version
        for name, version in desired.items()
        if current_packages.get(name) != version
    }
    if not changed:
        return [], []

    if "archives" in snapshot:
        matching_archives = {}
        missing = []
        for package_name in changed:
            archive = snapshot["archives"].get(package_name)
            if not isinstance(archive, dict):
                missing.append(package_name)
                continue
            blob_name = archive.get("blob")
            expected_digest = archive.get("sha256")
            if (
                not isinstance(blob_name, str)
                or Path(blob_name).name != blob_name
                or not isinstance(expected_digest, str)
                or not re.fullmatch(r"[a-f0-9]{64}", expected_digest)
                or not blob_name.startswith(expected_digest)
            ):
                raise RestoreError(f"Snapshot archive metadata is invalid for {package_name}")
            archive_path = librarian.SNAPSHOT_ROOT / "blobs" / blob_name
            if archive_path.is_symlink() or not archive_path.is_file():
                missing.append(package_name)
                continue
            try:
                if librarian._sha256(archive_path) != expected_digest:
                    raise RestoreError(
                        f"Snapshot package archive checksum mismatch for {package_name}"
                    )
            except OSError as error:
                raise RestoreError(f"Cannot read snapshot package archive: {error}") from error
            matching_archives[package_name] = archive_path
        return list(matching_archives.values()), missing

    if extraction_path is None:
        return [], sorted(changed)

    package_manager = snapshot["target"]["package_manager"]
    cache_root = extraction_path / "package-cache"
    if not cache_root.is_dir():
        cache_root = extraction_path

    matching_archives = {}
    suffixes = {
        "apt": (".deb",),
        "pacman": (".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar"),
        "dnf": (".rpm",),
    }[package_manager]
    separators = ("_", "-") if package_manager == "apt" else ("-",)
    candidate_prefixes = tuple(
        package_name.split(":", 1)[0] + separator
        for package_name in changed
        for separator in separators
    )
    for archive_path in sorted(cache_root.rglob("*")):
        if not archive_path.is_file() or not archive_path.name.endswith(suffixes):
            continue
        if not archive_path.name.startswith(candidate_prefixes):
            continue
        identity = _archive_package(package_manager, archive_path)
        if identity is None:
            continue
        package, version = identity
        package_names = (package, package.split(":", 1)[0])
        matching_name = next(
            (
                name for name in package_names
                if name in changed and changed[name] == version
            ),
            None,
        )
        if matching_name is not None:
            matching_archives[matching_name] = archive_path

    missing = sorted(set(changed) - set(matching_archives))
    return list(matching_archives.values()), missing


def _target_command(target, command):
    """Prefix a command for execution inside an LXC target."""

    if target["kind"] == "lxc":
        return ["pct", "exec", str(target["id"]), "--", *command]
    return command


def _restore_command(snapshot, archive_paths):
    """Build the native package-manager command for cached package files."""

    manager = snapshot["target"]["package_manager"]
    if manager == "pacman":
        return ["pacman", "-U", "--noconfirm", *map(str, archive_paths)]
    if manager == "apt":
        return [
            "apt-get", "install", "-y", "--allow-downgrades", "--",
            *map(str, archive_paths),
        ]
    if manager == "dnf":
        return ["dnf", "install", "-y", *map(str, archive_paths)]
    raise RestoreError(f"Unsupported package manager: {manager}")


def _restore_inside_lxc(snapshot, archive_paths):
    """Copy selected package archives into a container and install them."""

    target = snapshot["target"]
    remote_directory = f"/tmp/pacmonster-restore-{uuid4().hex}"
    setup = subprocess.run(
        ["pct", "exec", target["id"], "--", "mkdir", "-p", remote_directory],
        capture_output=True,
        text=True,
        check=False,
    )
    if setup.returncode != 0:
        return setup

    remote_archives = []
    try:
        for archive_path in archive_paths:
            remote_path = f"{remote_directory}/{archive_path.name}"
            copied = subprocess.run(
                ["pct", "push", target["id"], str(archive_path), remote_path],
                capture_output=True,
                text=True,
                check=False,
            )
            if copied.returncode != 0:
                return copied
            remote_archives.append(remote_path)

        return subprocess.run(
            _target_command(target, _restore_command(snapshot, remote_archives)),
            capture_output=True,
            text=True,
            check=False,
        )
    finally:
        subprocess.run(
            [
                "pct", "exec", target["id"], "--",
                "rm", "-rf", "--", remote_directory,
            ],
            capture_output=True,
            text=True,
            check=False,
        )


def _restore_snapshot(snapshot_id, assume_yes=False, json_output=False):
    """Restore changed packages from an exact-version snapshot archive.

    Returns a result dictionary with the process status, restored package names,
    and any packages unavailable in the saved cache.
    """

    snapshot = librarian.load_snapshot(snapshot_id)
    target = snapshot["target"]
    extraction_path = _extract_cache(snapshot)
    try:
        current_packages = librarian.package_manifest(target)
        added_after_snapshot = sorted(
            set(current_packages) - set(snapshot["packages"])
        )
        archive_paths, missing = _find_restore_packages(
            snapshot,
            extraction_path,
            current_packages,
        )
        if missing:
            return {
                "status": "missing",
                "restored": [],
                "missing": missing,
                "added_after_snapshot": added_after_snapshot,
                "target": target["name"],
            }
        if not archive_paths:
            return {
                "status": "unchanged",
                "restored": [],
                "missing": [],
                "added_after_snapshot": added_after_snapshot,
                "target": target["name"],
            }

        if not json_output:
            print(f"Target: {target['name']}")
            print(f"Packages to restore: {len(archive_paths)}")
            for archive_path in archive_paths:
                print(f"  {archive_path.name}")
        if not assume_yes:
            prompt = "Restore these packages? [y/N] "
            if json_output:
                print(prompt, end="", file=sys.stderr, flush=True)
                confirmed = input().lower() == "y"
            else:
                confirmed = input(prompt).lower() == "y"
            if not confirmed:
                return {
                    "status": "aborted",
                    "restored": [],
                    "missing": [],
                    "added_after_snapshot": added_after_snapshot,
                    "target": target["name"],
                }

        if target["kind"] == "lxc":
            result = _restore_inside_lxc(snapshot, archive_paths)
        else:
            result = subprocess.run(
                _restore_command(snapshot, archive_paths),
                capture_output=True,
                text=True,
                check=False,
            )

        return {
            "status": "success" if result.returncode == 0 else "failed",
            "restored": [path.name for path in archive_paths],
            "missing": [],
            "added_after_snapshot": added_after_snapshot,
            "target": target["name"],
            "returncode": result.returncode,
            "stdout": result.stdout,
            "stderr": result.stderr,
        }
    finally:
        if extraction_path is not None:
            shutil.rmtree(extraction_path, ignore_errors=True)


def restore_snapshot(snapshot_id, assume_yes=False, json_output=False):
    """Serialize restoration against Pacmonster updates and run the restore."""

    if not acquire_lock():
        return {
            "status": "locked",
            "restored": [],
            "missing": [],
            "target": "",
        }
    try:
        return _restore_snapshot(
            snapshot_id,
            assume_yes=assume_yes,
            json_output=json_output,
        )
    finally:
        release_lock()


def render_result(result):
    """Print a restore outcome and relevant package-manager diagnostics."""

    status = result["status"]
    if status == "missing":
        print(f"Cannot restore {result['target']}; cached versions are missing:")
        for package in result["missing"]:
            print(f"  {package}")
        return
    if status == "unchanged":
        print(f"{result['target']} already matches the snapshot.")
        return
    if status == "aborted":
        print("Restore cancelled.")
        return
    if status == "locked":
        print("Another Pacmonster update or restore is already running.")
        return

    if result.get("stdout"):
        print(result["stdout"], end="")
    if result.get("stderr"):
        print(result["stderr"], end="")
    if status == "success":
        print(f"Restored {len(result['restored'])} packages on {result['target']}.")
    else:
        print(f"Restore failed on {result['target']} (exit {result['returncode']}).")
    if result.get("added_after_snapshot"):
        print(
            "Packages installed after the snapshot were left unchanged: "
            + ", ".join(result["added_after_snapshot"])
        )
