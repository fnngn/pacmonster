"""Create and manage package-state snapshots for update recovery."""

import json
import hashlib
import os
from pathlib import Path
import re
import shutil
import subprocess
import sys
import tempfile
from datetime import datetime, timezone
from uuid import uuid4


SNAPSHOT_ROOT = Path("/var/lib/pacmonster/snapshots")
RETENTION_COUNT = 3
PACKAGE_COMMANDS = {
    "apt": ["dpkg-query", "-W", "-f=${binary:Package}\t${Version}\n"],
    "pacman": ["pacman", "-Q"],
    "dnf": ["rpm", "-qa", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}.%{ARCH}\n"],
}
CACHE_DIRECTORIES = {
    "apt": "/var/cache/apt/archives",
    "pacman": "/var/cache/pacman/pkg",
    "dnf": "/var/cache/dnf",
}
PACKAGE_ARCHIVE_SUFFIXES = {
    "apt": (".deb",),
    "pacman": (".pkg.tar.zst", ".pkg.tar.xz", ".pkg.tar.gz", ".pkg.tar"),
    "dnf": (".rpm",),
}


class SnapshotError(RuntimeError):
    """Raised when a complete package snapshot cannot be recorded."""


def _require_snapshot_root():
    """Create the snapshot store or fail with an actionable permission error."""

    try:
        SNAPSHOT_ROOT.mkdir(parents=True, exist_ok=True)
    except OSError as error:
        raise SnapshotError(f"Cannot create snapshot store {SNAPSHOT_ROOT}: {error}") from error

    if not os.access(SNAPSHOT_ROOT, os.W_OK):
        raise SnapshotError(f"Snapshot store is not writable: {SNAPSHOT_ROOT}")


def _target_command(target, command):
    """Prefix commands for LXC targets, leaving host commands unchanged."""

    if target["kind"] == "lxc":
        return ["pct", "exec", str(target["id"]), "--", *command]
    return command


def package_manifest(target):
    """Return installed package versions keyed by package name."""

    package_manager = target["package_manager"]
    command = PACKAGE_COMMANDS.get(package_manager)
    if command is None:
        raise SnapshotError(f"Unsupported package manager: {package_manager}")

    result = subprocess.run(
        _target_command(target, command),
        text=True,
        capture_output=True,
        check=False,
    )
    if result.returncode != 0:
        raise SnapshotError(
            f"Could not read installed packages for {target['name']}: "
            f"{result.stderr.strip() or result.returncode}"
        )

    packages = {}
    for line in result.stdout.splitlines():
        fields = line.split("\t", 1) if package_manager != "pacman" else line.split()
        if len(fields) >= 2:
            packages[fields[0]] = fields[1]
    if not packages:
        raise SnapshotError(f"Package manifest for {target['name']} is empty")
    return packages


def _archive_identity(package_manager, archive_path):
    """Read the package name, version, and architecture from a native archive."""

    if package_manager == "pacman":
        command = ["pacman", "-Qp", str(archive_path)]
    elif package_manager == "apt":
        command = [
            "dpkg-deb", "-f", str(archive_path),
            "Package", "Version", "Architecture",
        ]
    else:
        command = [
            "rpm", "-qp", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}.%{ARCH}\n",
            str(archive_path),
        ]

    try:
        result = subprocess.run(
            command,
            capture_output=True,
            text=True,
            check=False,
        )
    except OSError as error:
        raise SnapshotError(f"Cannot inspect cached package {archive_path}: {error}") from error
    if result.returncode != 0:
        raise SnapshotError(
            f"Cannot inspect cached package {archive_path}: "
            f"{result.stderr.strip() or result.returncode}"
        )

    if package_manager == "apt":
        fields = result.stdout.splitlines()
    elif package_manager == "pacman":
        fields = result.stdout.split()
    else:
        fields = result.stdout.strip().split("\t")
    if package_manager == "apt" and len(fields) >= 3:
        return fields[0], fields[1], fields[2]
    if package_manager in ("pacman", "dnf") and len(fields) == 2:
        return fields[0], fields[1], None
    return None


def _manifest_package_name(name, version, architecture, packages):
    """Return the manifest key matching an archive's exact package identity."""

    candidates = [name]
    if architecture:
        candidates.insert(0, f"{name}:{architecture}")
    return next(
        (
            package_name for package_name in candidates
            if packages.get(package_name) == version
        ),
        None,
    )


def _matching_cache_archive_map(package_manager, archives, packages):
    """Map installed package names to cached archives with exact versions."""

    package_names = tuple(
        dict.fromkeys(name.split(":", 1)[0] for name in packages)
    )
    separator = "_" if package_manager == "apt" else "-"
    prefixes = tuple(f"{name}{separator}" for name in package_names)
    candidates = [
        path for path in archives
        if path.name.startswith(prefixes)
    ]
    matching = {}

    if package_manager == "pacman":
        for offset in range(0, len(candidates), 100):
            batch = candidates[offset:offset + 100]
            try:
                result = subprocess.run(
                    ["pacman", "-Qp", *map(str, batch)],
                    capture_output=True,
                    text=True,
                    check=False,
                )
            except OSError as error:
                raise SnapshotError(f"Cannot inspect cached pacman packages: {error}") from error
            identities = result.stdout.splitlines()
            if result.returncode != 0 or len(identities) != len(batch):
                raise SnapshotError(
                    "Cannot reliably inspect cached pacman packages: "
                    f"{result.stderr.strip() or result.returncode}"
                )
            for archive_path, identity in zip(batch, identities):
                fields = identity.split(maxsplit=1)
                if len(fields) == 2 and packages.get(fields[0]) == fields[1]:
                    matching[fields[0]] = archive_path
        return matching

    for archive_path in candidates:
        identity = _archive_identity(package_manager, archive_path)
        if identity is None:
            continue
        name, version, architecture = identity
        package_name = _manifest_package_name(name, version, architecture, packages)
        if package_name is not None:
            matching[package_name] = archive_path
    return matching


def _matching_cache_archives(package_manager, archives, packages):
    """Return cached archives that exactly match versions in the manifest."""

    return list(_matching_cache_archive_map(package_manager, archives, packages).values())


def _store_archive_blob(archive_path):
    """Store one package archive once by content hash and return its blob record."""

    blob_directory = SNAPSHOT_ROOT / "blobs"
    blob_directory.mkdir(parents=True, exist_ok=True)
    digest = _sha256(archive_path, show_progress=True)
    blob_name = f"{digest}{''.join(archive_path.suffixes)}"
    blob_path = blob_directory / blob_name
    created = False
    if not blob_path.exists():
        temporary_path = None
        try:
            with tempfile.NamedTemporaryFile(
                dir=blob_directory,
                prefix=".package-",
                delete=False,
            ) as temporary:
                temporary_path = Path(temporary.name)
                with archive_path.open("rb") as source:
                    total_bytes = archive_path.stat().st_size
                    copied_bytes = 0
                    next_report = 256 * 1024**2
                    while block := source.read(1024 * 1024):
                        temporary.write(block)
                        copied_bytes += len(block)
                        if copied_bytes >= next_report and copied_bytes < total_bytes:
                            print(
                                f"Copying {archive_path.name}: "
                                f"{copied_bytes / 1024**3:.1f}/"
                                f"{total_bytes / 1024**3:.1f} GiB",
                                file=sys.stderr,
                                flush=True,
                            )
                            next_report += 256 * 1024**2
            os.link(temporary_path, blob_path)
            created = True
        except FileExistsError:
            if blob_path.exists():
                pass
            else:
                raise
        finally:
            if temporary_path is not None:
                temporary_path.unlink(missing_ok=True)
    return {
        "blob": blob_name,
        "sha256": digest,
        "bytes": archive_path.stat().st_size,
        "new": created,
    }


def _archive_identity_for_target(target, archive_path):
    """Read a package identity locally or inside an LXC container."""

    if target["kind"] == "host":
        return _archive_identity(target["package_manager"], archive_path)

    package_manager = target["package_manager"]
    command = {
        "apt": [
            "dpkg-deb", "-f", str(archive_path),
            "Package", "Version", "Architecture",
        ],
        "pacman": ["pacman", "-Qp", str(archive_path)],
        "dnf": [
            "rpm", "-qp", "--qf", "%{NAME}\t%{VERSION}-%{RELEASE}.%{ARCH}\n",
            str(archive_path),
        ],
    }[package_manager]
    result = subprocess.run(
        _target_command(target, command),
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    if package_manager == "apt":
        fields = result.stdout.splitlines()
        if len(fields) >= 3:
            return fields[0], fields[1], fields[2]
    elif package_manager == "pacman":
        fields = result.stdout.split()
        if len(fields) >= 2:
            return fields[0], fields[1], None
    else:
        fields = result.stdout.strip().split("\t")
        if len(fields) == 2:
            return fields[0], fields[1], None
    return None


def _archive_cache(target, packages):
    """Store only exact-version cached package files, deduplicating by hash."""

    package_manager = target["package_manager"]
    cache_directory = CACHE_DIRECTORIES[package_manager]
    suffixes = PACKAGE_ARCHIVE_SUFFIXES[package_manager]
    stored = {}
    previous_snapshots = list_snapshots(target_id=target["id"])
    previous_snapshot = next(
        (
            snapshot for snapshot in previous_snapshots
            if snapshot.get("target", {}).get("kind") == target["kind"]
        ),
        None,
    )
    packages_to_find = dict(packages)
    if previous_snapshot is not None:
        previous_archives = previous_snapshot.get("archives", {})
        previous_packages = previous_snapshot.get("packages", {})
        blob_directory = SNAPSHOT_ROOT / "blobs"
        for package_name, version in tuple(packages_to_find.items()):
            archive = previous_archives.get(package_name)
            if previous_packages.get(package_name) != version or not isinstance(archive, dict):
                continue
            blob_name = archive.get("blob")
            digest = archive.get("sha256")
            if (
                not isinstance(blob_name, str)
                or Path(blob_name).name != blob_name
                or not isinstance(digest, str)
                or not re.fullmatch(r"[a-f0-9]{64}", digest)
                or not blob_name.startswith(digest)
            ):
                continue
            blob_path = blob_directory / blob_name
            if (
                not blob_path.is_file()
                or blob_path.is_symlink()
                or blob_path.stat().st_size != archive.get("bytes")
            ):
                continue
            stored[package_name] = {**archive, "new": False}
            del packages_to_find[package_name]
    if not packages_to_find:
        print(
            f"[{target['name']}] reusing all {len(stored)} package archives "
            "from the previous snapshot",
            file=sys.stderr,
            flush=True,
        )
        return stored

    if target["kind"] == "host":
        cache_path = Path(cache_directory)
        if not cache_path.is_dir():
            return stored
        package_archives = [
            path for path in cache_path.rglob("*")
            if path.is_file()
            and not path.is_symlink()
            and path.name.endswith(suffixes)
        ]
        matching = _matching_cache_archive_map(
            package_manager,
            package_archives,
            packages_to_find,
        )
        if not matching:
            return stored
        total_bytes = sum(path.stat().st_size for path in matching.values())
        print(
            f"[{target['name']}] saving {len(matching)} matching cached package "
            f"archives ({total_bytes / 1024**3:.1f} GiB); press Ctrl+C to cancel",
            file=sys.stderr,
            flush=True,
        )
        for index, (package_name, package_archive) in enumerate(matching.items(), start=1):
            print(
                f"[{target['name']}] saving {package_name} "
                f"({package_archive.stat().st_size / 1024**2:.0f} MiB)",
                file=sys.stderr,
                flush=True,
            )
            stored[package_name] = _store_archive_blob(package_archive)
            print(
                f"[{target['name']}] snapshot packages {index}/{len(matching)}",
                file=sys.stderr,
                flush=True,
            )
        new_bytes = sum(item["bytes"] for item in stored.values() if item["new"])
        print(
            f"[{target['name']}] newly stored package data: "
            f"{new_bytes / 1024**3:.2f} GiB",
            file=sys.stderr,
            flush=True,
        )
        return stored

    shared_blobs = _shared_blob_index(package_manager)
    exists = subprocess.run(
        ["pct", "exec", str(target["id"]), "--", "test", "-d", cache_directory],
        capture_output=True,
        check=False,
    )
    if exists.returncode != 0:
        return stored

    result = subprocess.run(
        ["pct", "exec", str(target["id"]), "--", "find", cache_directory, "-type", "f"],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        raise SnapshotError(
            f"Could not list package cache for {target['name']}: "
            f"{result.stderr.strip() or result.returncode}"
        )
    package_names = tuple(name.split(":", 1)[0] for name in packages_to_find)
    separator = "_" if package_manager == "apt" else "-"
    prefixes = tuple(f"{name}{separator}" for name in package_names)
    candidates = [
        Path(path)
        for path in result.stdout.splitlines()
        if Path(path).name.endswith(suffixes)
        and Path(path).name.startswith(prefixes)
    ]
    matching = {}
    for index, candidate in enumerate(candidates, start=1):
        if index == 1 or index % 10 == 0:
            print(
                f"[{target['name']}] inspecting container cache "
                f"{index}/{len(candidates)} candidates; press Ctrl+C to cancel",
                file=sys.stderr,
                flush=True,
            )
        identity = _archive_identity_for_target(target, candidate)
        if identity is None:
            continue
        package_name = _manifest_package_name(*identity, packages_to_find)
        if package_name is not None:
            _, version, _ = identity
            reused = _reuse_remote_blob(
                target,
                candidate,
                package_name,
                version,
                shared_blobs,
            )
            if reused is not None:
                stored[package_name] = reused
            else:
                matching[package_name] = candidate
    if candidates:
        print(
            f"[{target['name']}] inspected {len(candidates)} container cache candidates",
            file=sys.stderr,
            flush=True,
        )

    with tempfile.TemporaryDirectory(prefix="pacmonster-package-") as temporary:
        for index, (package_name, remote_path) in enumerate(matching.items(), start=1):
            local_path = Path(temporary) / Path(remote_path).name
            pulled = subprocess.run(
                ["pct", "pull", str(target["id"]), str(remote_path), str(local_path)],
                capture_output=True,
                text=True,
                check=False,
            )
            if pulled.returncode != 0:
                raise SnapshotError(
                    f"Could not copy cached package from {target['name']}: "
                    f"{pulled.stderr.strip() or pulled.returncode}"
                )
            stored[package_name] = _store_archive_blob(local_path)
            if index % 25 == 0 or index == len(matching):
                print(
                    f"[{target['name']}] saved container packages "
                    f"{index}/{len(matching)}",
                    flush=True,
                )
    return stored


def _shared_blob_index(package_manager):
    """Index retained package blobs by manager, package name, and version."""

    index = {}
    blob_directory = SNAPSHOT_ROOT / "blobs"
    for snapshot in list_snapshots():
        if snapshot.get("target", {}).get("package_manager") != package_manager:
            continue
        packages = snapshot.get("packages", {})
        archives = snapshot.get("archives", {})
        for package_name, archive in archives.items():
            if not isinstance(archive, dict):
                continue
            blob_name = archive.get("blob")
            digest = archive.get("sha256")
            version = packages.get(package_name)
            if (
                not isinstance(blob_name, str)
                or Path(blob_name).name != blob_name
                or not isinstance(digest, str)
                or not re.fullmatch(r"[a-f0-9]{64}", digest)
                or not blob_name.startswith(digest)
                or not version
            ):
                continue
            blob_path = blob_directory / blob_name
            if (
                not blob_path.is_file()
                or blob_path.is_symlink()
                or blob_path.stat().st_size != archive.get("bytes")
            ):
                continue
            index.setdefault((package_name, version), {})[digest] = {
                **archive,
                "new": False,
            }
    return index


def _reuse_remote_blob(target, remote_path, package_name, version, shared_blobs):
    """Reuse an identical retained blob without pulling it from an LXC target."""

    candidates = shared_blobs.get((package_name, version), {})
    if not candidates:
        return None
    result = subprocess.run(
        [
            "pct", "exec", str(target["id"]), "--",
            "sha256sum", "--", str(remote_path),
        ],
        capture_output=True,
        text=True,
        check=False,
    )
    if result.returncode != 0:
        return None
    fields = result.stdout.split(maxsplit=1)
    if not fields:
        return None
    return candidates.get(fields[0])


def _snapshot_id(target):
    """Build a filesystem-safe, unique snapshot identifier."""

    target_id = re.sub(r"[^A-Za-z0-9_.-]+", "-", str(target["id"]))
    timestamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    return f"{target['kind']}-{target_id}-{timestamp}-{uuid4().hex[:8]}"


def _sha256(path, show_progress=False):
    """Return a streaming SHA-256 digest for a potentially large cache archive."""

    digest = hashlib.sha256()
    with path.open("rb") as archive:
        total_bytes = path.stat().st_size
        processed_bytes = 0
        next_report = 256 * 1024**2
        if show_progress and total_bytes >= next_report:
            print(
                f"Checking archive content {path.name} "
                f"({total_bytes / 1024**3:.1f} GiB); press Ctrl+C to cancel",
                file=sys.stderr,
                flush=True,
            )
        for block in iter(lambda: archive.read(1024 * 1024), b""):
            digest.update(block)
            processed_bytes += len(block)
            if show_progress and processed_bytes >= next_report and processed_bytes < total_bytes:
                print(
                    f"Checking {path.name}: "
                    f"{processed_bytes / 1024**3:.1f}/"
                    f"{total_bytes / 1024**3:.1f} GiB",
                    file=sys.stderr,
                    flush=True,
                )
                next_report += 256 * 1024**2
    return digest.hexdigest()


def _cleanup_failed_snapshot(snapshot_directory):
    """Remove partial snapshot data and any newly orphaned package blobs."""

    shutil.rmtree(snapshot_directory, ignore_errors=True)
    try:
        prune_blobs()
    except (OSError, SnapshotError) as error:
        print(
            f"Warning: could not clean unreferenced package blobs: {error}",
            file=sys.stderr,
            flush=True,
        )


def create_snapshot(target):
    """Save a package manifest and deduplicated cached archives for one target."""

    _require_snapshot_root()
    snapshot_id = _snapshot_id(target)
    snapshot_directory = SNAPSHOT_ROOT / snapshot_id

    try:
        snapshot_directory.mkdir()
        packages = package_manifest(target)
        archives = _archive_cache(target, packages)
        metadata = {
            "id": snapshot_id,
            "created": datetime.now(timezone.utc).isoformat(),
            "target": {
                "id": str(target["id"]),
                "name": target["name"],
                "kind": target["kind"],
                "distro": target.get("distro", "unknown"),
                "package_manager": target["package_manager"],
            },
            "package_count": len(packages),
            "packages": packages,
            "archives": archives,
            "archive_count": len(archives),
            "archive_bytes": sum(item["bytes"] for item in archives.values()),
            "new_archive_bytes": sum(
                item["bytes"] for item in archives.values() if item["new"]
            ),
            "restorable": bool(archives),
        }
        metadata_path = snapshot_directory / "snapshot.json"
        temporary_path = snapshot_directory / "snapshot.json.tmp"
        temporary_path.write_text(json.dumps(metadata, indent=2) + "\n")
        temporary_path.replace(metadata_path)
    except KeyboardInterrupt:
        _cleanup_failed_snapshot(snapshot_directory)
        print(
            f"[{target['name']}] snapshot cancelled; temporary snapshot data cleaned",
            file=sys.stderr,
            flush=True,
        )
        raise
    except Exception as error:
        _cleanup_failed_snapshot(snapshot_directory)
        if isinstance(error, SnapshotError):
            raise
        raise SnapshotError(f"Could not save snapshot for {target['name']}: {error}") from error

    prune_snapshots(target)
    return metadata


def list_snapshots(target_id=None):
    """Return snapshot metadata newest first, optionally filtered by target ID."""

    if not SNAPSHOT_ROOT.is_dir():
        return []

    snapshots = []
    for metadata_path in SNAPSHOT_ROOT.glob("*/snapshot.json"):
        try:
            metadata = json.loads(metadata_path.read_text())
        except (OSError, json.JSONDecodeError) as error:
            raise SnapshotError(f"Cannot read snapshot metadata {metadata_path}: {error}") from error
        if (
            not isinstance(metadata, dict)
            or not isinstance(metadata.get("target"), dict)
            or not isinstance(metadata.get("packages"), dict)
            or not metadata.get("id")
            or not metadata.get("created")
        ):
            raise SnapshotError(f"Snapshot metadata is incomplete: {metadata_path}")
        if target_id is None or metadata.get("target", {}).get("id") == str(target_id):
            metadata["path"] = str(metadata_path.parent)
            snapshots.append(metadata)
    return sorted(snapshots, key=lambda item: item.get("created", ""), reverse=True)


def load_snapshot(snapshot_id):
    """Load and validate snapshot metadata by its opaque snapshot ID."""

    if not re.fullmatch(r"[A-Za-z0-9_.-]+", snapshot_id):
        raise SnapshotError("Invalid snapshot ID")
    snapshot_directory = (SNAPSHOT_ROOT / snapshot_id).resolve()
    if snapshot_directory.parent != SNAPSHOT_ROOT.resolve():
        raise SnapshotError("Snapshot path is outside the snapshot store")
    metadata_path = snapshot_directory / "snapshot.json"
    try:
        metadata = json.loads(metadata_path.read_text())
    except FileNotFoundError as error:
        raise SnapshotError(f"Snapshot not found: {snapshot_id}") from error
    except json.JSONDecodeError as error:
        raise SnapshotError(f"Snapshot metadata is invalid: {snapshot_id}") from error
    except OSError as error:
        raise SnapshotError(f"Cannot read snapshot metadata {snapshot_id}: {error}") from error
    if (
        not isinstance(metadata, dict)
        or metadata.get("id") != snapshot_id
        or not isinstance(metadata.get("packages"), dict)
        or ("archives" in metadata and not isinstance(metadata["archives"], dict))
    ):
        raise SnapshotError(f"Snapshot metadata is incomplete: {snapshot_id}")
    target = metadata.get("target")
    if not isinstance(target, dict):
        raise SnapshotError(f"Snapshot target metadata is invalid: {snapshot_id}")
    if (
        target.get("kind") not in ("host", "lxc")
        or target.get("package_manager") not in PACKAGE_COMMANDS
        or not target.get("id")
        or not target.get("name")
    ):
        raise SnapshotError(f"Snapshot target metadata is invalid: {snapshot_id}")
    archive_name = metadata.get("archive")
    if archive_name is not None and Path(archive_name).name != archive_name:
        raise SnapshotError(f"Snapshot archive path is invalid: {snapshot_id}")
    metadata["path"] = str(snapshot_directory)
    return metadata


def prune_snapshots(target):
    """Keep only the newest RETENTION_COUNT snapshots for one target."""

    snapshots = list_snapshots(target_id=target["id"])
    target_snapshots = [
        snapshot for snapshot in snapshots
        if snapshot.get("target", {}).get("kind") == target["kind"]
    ]
    for snapshot in target_snapshots[RETENTION_COUNT:]:
        shutil.rmtree(snapshot["path"])
    prune_blobs()


def prune_blobs():
    """Remove package blobs no retained snapshot references."""

    blob_directory = SNAPSHOT_ROOT / "blobs"
    if not blob_directory.is_dir():
        return
    referenced = set()
    for snapshot in list_snapshots():
        for archive in snapshot.get("archives", {}).values():
            if isinstance(archive, dict) and isinstance(archive.get("blob"), str):
                referenced.add(archive["blob"])
    for blob_path in blob_directory.iterdir():
        if blob_path.is_file() and blob_path.name not in referenced:
            blob_path.unlink()
