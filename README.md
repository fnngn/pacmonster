# Pacmonster

Pacmonster is a Linux and Proxmox update orchestrator for homelabs and small
infrastructure environments. Its task-focused terminal interfaces put updates,
diagnostics, inventory, logs, and package recovery behind one consistent
command-line application.

It uses native package managers rather than replacing them. Pacmonster has no
daemon, web UI, or application-owned database engine. Its caches, history,
runtime state, event logs, and recovery snapshots are local files, including
JSON and JSON Lines files; native package managers retain and manage their own
package databases.

```text
Start
 ├─ Ghosthunter    Update systems
 ├─ Witchdoctor    Diagnose systems and the installation
 ├─ Librarian      Browse inventory and package snapshots
 ├─ Archeologist   Search logs
 ├─ Restorer       Recover package versions from snapshots
 └─ Advisor        Read help, the handbook, and project information
```

Current release: **1.0.0**. See the [1.0.0 release notes](RELEASE_NOTES_1.0.0.md)
for included features, supported scope, and project direction.

## Contents

- [Interfaces](#interfaces)
  - [Ghosthunter and updates](#ghosthunter-and-updates)
  - [Other interface actions](#other-interface-actions)
- [Installation](#installation)
- [CLI reference](#cli-reference)
- [Supported systems](#supported-systems)
- [Snapshots and recovery](#snapshots-and-recovery)
- [Update scheduling and safety](#update-scheduling-and-safety)
- [Project direction and roadmap](#project-direction-and-roadmap)
- [Local data](#local-data)
- [Development](#development)
- [License](#license)

## Interfaces

Open the main menu with:

```bash
pacmonster start
```

| Interface | Command | What it does |
| --- | --- | --- |
| **Ghosthunter** | `sudo pacmonster ghosthunter` | Choose to run all planned updates, run with package-manager confirmation, or preview planned actions. |
| **Witchdoctor** | `sudo pacmonster witchdoctor` | Run system diagnostics, check the Pacmonster installation, refresh cached status, and view compact or detailed status. |
| **Librarian** | `sudo pacmonster librarian` | Browse and filter snapshots, create snapshots, refresh or view inventory, and read update history. |
| **Archeologist** | `sudo pacmonster archeologist` | Browse package-manager logs, filter likely warnings/errors, or search log text. |
| **Restorer** | `sudo pacmonster restorer` | Inspect snapshots and restore package versions after an explicit confirmation. |
| **Advisor** | `pacmonster advisor` | Browse command help, this README, the license, and developer information. |

The interfaces use parchment as the main color with petrol and gold accents.
Use `↑`/`↓` or `j`/`k` to navigate, `Enter` to choose, `Esc` to return, and
`q` to leave. Long text panes support `PgUp`/`PgDn`, `Home`, and `End`.

### Ghosthunter and updates

Running `pacmonster ghosthunter` opens Ghosthunter's action menu. **Run all
updates** skips Pacmonster's extra confirmation prompt; **Run with
confirmation** asks before starting the update run; **Preview update** displays
the planned actions without applying them. After an update, Ghosthunter lists
systems whose installed packages changed and shows each package's previous and
current version. It also identifies failed systems and any package-inspection
errors.

To launch the update view with confirmation directly:

```bash
sudo pacmonster ghosthunter --confirm
```

The ordinary update commands are also available:

```bash
sudo pacmonster                 # Interactive update
sudo pacmonster update          # Interactive update
sudo pacmonster update --yes    # Do not ask before starting
sudo pacmonster update --yes --json > update-report.json
sudo pacmonster dry-run         # Preview planned commands
```

`update --json` writes a versioned JSON report to stdout; progress and native
package-manager output go to stderr so the report remains directly parseable.
The report includes changed packages, old/new versions, system success and
failure details, and a summary. Each attempted system includes its name, ID,
system type, distribution, package manager, and running kernel when available.
Redirect stdout to a file or pipe it to another program. A successful
package-manager run with no detected package changes is reported separately
from systems that actually changed packages.
Each package change has a `change` value (`updated`, `installed`, or `removed`)
and nullable `previous_version` / `current_version` fields. The report's
`updated_systems` contains successful systems with detected changes;
`failed_systems` identifies failed targets, and `systems` contains every
attempted target.

```json
{
  "schema_version": 1,
  "generated_at": "2026-10-25T12:00:00+00:00",
  "summary": {
    "systems_attempted": 1,
    "systems_succeeded": 1,
    "systems_failed": 0,
    "packages_changed": 1
  },
  "updated_systems": [
    {
      "id": "101",
      "name": "web",
      "kind": "lxc",
      "distro": "Debian",
      "package_manager": "apt",
      "success": true,
      "returncode": 0,
      "duration_seconds": 12.5,
      "snapshot_id": "snapshot-id",
      "packages": [{
        "name": "libdemo",
        "change": "updated",
        "previous_version": "1.0",
        "current_version": "2.0"
      }],
      "package_report_error": null
    }
  ],
  "failed_systems": [],
  "systems": [
    {
      "id": "101",
      "name": "web",
      "kind": "lxc",
      "distro": "Debian",
      "package_manager": "apt",
      "success": true,
      "returncode": 0,
      "duration_seconds": 12.5,
      "snapshot_id": "snapshot-id",
      "packages": [{
        "name": "libdemo",
        "change": "updated",
        "previous_version": "1.0",
        "current_version": "2.0"
      }],
      "package_report_error": null
    }
  ]
}
```

During an update, Ghosthunter streams package-manager output and shows the
worker pool. Pressing `q` or `Ctrl+C` requests a safe interrupt; Pacmonster
waits for package-manager processes to stop and escalates only if necessary.

### Other interface actions

The Librarian's menu offers the same inventory and history functions as the
CLI. Its snapshot browser can show every snapshot or filter by target ID.

The Witchdoctor separates live diagnostics and installation checks from cached
status inspection. `Refresh Status` updates the cache before the status views
are opened.

The Archeologist can show all available Pacmonster and package-manager log
records, reduce results to likely warnings and errors, or search by text.

Restorer only restores snapshots that include the required package archives.
It previews the changes and requires confirmation before installing packages.
`--yes` is available for scripted restores; see [Snapshots and recovery](#snapshots-and-recovery).

## Installation

Pacmonster targets Linux hosts with Python 3.11 or newer. Install system-wide
from a checkout:

```bash
git clone https://github.com/fnngn/pacmonster.git
cd pacmonster
./install.sh
```

The installer installs both `pacmonster` and its TUI. For published releases on
x86_64 and ARM64 Linux, you can optionally install the rolling prebuilt TUI:

```bash
./install.sh --prebuilt-tui
```

By default, the installer builds the TUI from source and can bootstrap
Rust/Cargo 1.85 or newer and native build tools. This avoids depending on
release binaries being kept in sync. Build artifacts are cached under
`/var/cache/pacmonster/cargo-target`, so later installs reuse dependencies and
only rebuild changed components. The optional prebuilt mode downloads the
rolling `latest` binary and verifies its checksum and version. The release
workflow refreshes those binaries on every push to `master` and on version tags.
The command is installed in `/usr/local/bin`, so `sudo pacmonster` does not rely
on a user's private executable path.

Check the installation:

```bash
sudo pacmonster doctor --installation
```

Package operations and Proxmox discovery require root privileges. Pacmonster
also needs the native package manager for each system it updates. On Proxmox,
`pct` and `qm` are used for container and VM discovery. VM update and status
support are not currently implemented.

## CLI reference

Every interface has a corresponding CLI command. `pacmonster help` prints the
tabular command reference with a short description for each function;
`pacmonster help --json` returns the command names as a JSON list.

| Command | Description |
| --- | --- |
| `pacmonster refresh` | Rebuild the system inventory cache. |
| `pacmonster inventory [--json]` | Show cached inventory. |
| `pacmonster refresh-status` | Refresh inventory and cached package status. |
| `pacmonster status [--verbose] [--json]` | Show cached status, optionally including package names. |
| `pacmonster doctor [--installation] [--json]` | Run system or installation checks. |
| `pacmonster dry-run [--json]` | Show planned update actions without applying them. |
| `pacmonster history [--json]` | Show recent maintenance history. |
| `pacmonster librarian list [TARGET_ID] [--json]` | List snapshots, optionally for one target. |
| `pacmonster librarian snapshot [TARGET_ID] [--json]` | Create snapshots for all targets or one target. |
| `pacmonster librarian show SNAPSHOT_ID [--json]` | Show snapshot details. |
| `pacmonster archeologist all` / `pacmonster archeologist issues` | Read all log records or likely issues; both accept `--json`. |
| `pacmonster archeologist search QUERY [--json]` | Search log records. |
| `pacmonster restorer list [--json]` | List snapshots available for recovery. |
| `pacmonster restorer restore SNAPSHOT_ID [--yes] [--json]` | Restore package versions from a snapshot. |

`--json` is intended for scripts and integrations. Commands that discover
systems, inspect package state, or change packages generally need `sudo`.

## Supported systems

Pacmonster discovers supported Linux systems on the host and, on Proxmox,
Linux containers.

| Distribution family | Package manager |
| --- | --- |
| Debian and Proxmox VE | APT |
| Arch Linux | pacman |
| Fedora | DNF |

VM discovery is available, but VM status checks and updates are not supported.

## Snapshots and recovery

Before updates, Pacmonster's Librarian saves each target's installed package
manifest and matching cached package archives. Snapshots are stored under
`/var/lib/pacmonster/snapshots/`. Package archives use content-addressed blobs,
so identical files may be shared between snapshots; a content hash is checked
before treating files as identical. The three newest manifests per target are
retained, and unreferenced blobs are removed.

Create and inspect snapshots:

```bash
sudo pacmonster librarian list
sudo pacmonster librarian snapshot [TARGET_ID]
sudo pacmonster librarian show SNAPSHOT_ID
sudo pacmonster restorer list
```

Restore a snapshot:

```bash
sudo pacmonster restorer restore SNAPSHOT_ID
sudo pacmonster restorer restore SNAPSHOT_ID --yes
```

Restorer compares installed versions with the selected snapshot and previews
the package changes. It stops if the complete set of exact-version archives is
not available rather than applying a partial restore. Manifest-only snapshots
are useful for reference but cannot restore package files. A restore changes
package versions only; it does not remove packages added later and is not a
backup of configuration, user data, or the filesystem. Snapshot creation needs
enough free disk space; `sudo pacmonster doctor` reports relevant checks.

## Update scheduling and safety

GhostPool schedules targets in FIFO order and reuses workers as they finish.
Worker capacity is derived from the CPUs available to Pacmonster, including
CPU affinity and detected cgroup quotas. It uses at most half of those cores,
with at least one worker when there is work, and has an overall limit of eight.
Available memory and current system load can further reduce the number of new
workers; already-running package operations are not interrupted just to lower
the worker count. A memory reserve and per-worker allowance also constrain
concurrency when memory information is available.

The update TUI estimates progress from recent successful runs for the same
target. Until there is enough history, it displays indeterminate progress
rather than inventing a percentage.

## Project direction and roadmap

Pacmonster is moving from its Python-only beginnings to a hybrid architecture:
the existing Python core continues to coordinate package managers and system
operations, while the terminal interface is built in Rust. This transition has
expanded the project from a collection of update scripts into a set of
interactive tools for updates, inventory, diagnostics, logs, and recovery.
It is a gradual evolution, not a claim that the Python core has already been
rewritten in Rust.

The TUI is becoming the primary guided experience, while the CLI remains a
first-class interface. Each operation should remain available to scripts and
operators without the TUI, with equivalent behavior and clear, versioned JSON
output as the integration contract. New interactive features should not make
automation depend on terminal parsing.

Planned areas of work, in priority order:

1. **System diagnostics and troubleshooting.** Extend Witchdoctor with a
   focused diagnostic workflow that gathers actionable host and service health
   information, explains findings, and offers safe next steps. Checks should be
   read-only by default, with any repair requiring an explicit action.
2. **Reliable CLI/TUI and JSON parity.** Keep command coverage aligned, document
   stable JSON schemas, and add regression checks so interactive and scripted
   paths remain equally useful.
3. **Docker and Compose lifecycle updates.** This is feasible, but it is not
   the same as upgrading packages inside a running container. The safe model
   is to pull a selected image, recreate its container or Compose service while
   preserving declared volumes and configuration, then check health and retain
   a rollback path. Pacmonster does not currently manage Docker containers;
   discovery, image policy, downtime, stateful volumes, and rollback need to be
   designed before enabling updates.
4. **Broader platform coverage.** Improve virtual-machine support and expand
   diagnostics and package-manager coverage only where behavior can be tested
   safely across supported distributions.
5. **Selective Rust migration.** Move core components to Rust only when it
   materially improves reliability, distribution, or maintainability. Preserve
   the existing CLI workflows and JSON contracts during any migration.

These are plans, not promises or currently supported capabilities. In
particular, Docker updates and VM updates are not available in this release.

## Local data

Inventory, status, and history caches live in
`~/.cache/pacmonster/` for the user running the command (with `sudo`, this is
typically root's home directory). Runtime state and event logs are written
under `/var/tmp/` with the effective user ID in their filenames.
These are file-backed stores, not a Pacmonster application database. Deleting
or editing them can remove Pacmonster's cached views and history, but does not
alter the native package manager's own package database.

| Data | Location |
| --- | --- |
| Inventory | `~/.cache/pacmonster/inventory.json` |
| Status | `~/.cache/pacmonster/status.json` |
| History | `~/.cache/pacmonster/history.json` |
| Update state | `/var/tmp/pacmonster-state-UID.json` |
| Update events | `/var/tmp/pacmonster-events-UID.jsonl` |
| Snapshot manifests and archives | `/var/lib/pacmonster/snapshots/` |

## Development

Run Python checks from the repository root:

```bash
python3 -m py_compile pacmonster/*.py
python3 -m unittest discover -s tests
```

Build and test the TUI:

```bash
cargo fmt --manifest-path tui/Cargo.toml -- --check
cargo test --manifest-path tui/Cargo.toml
cargo build --release --manifest-path tui/Cargo.toml
```

For local TUI development, `cargo run --manifest-path tui/Cargo.toml` opens the
dashboard. Its maintenance modes can be tested with `--run-ghosthunter`,
`--run-doctor`, `--run-librarian`, `--run-archeologist`, `--run-restorer`, or
`--run-advisor`. Running the installer again after source changes updates the
system-wide installation.

## License

Pacmonster is licensed under the GNU General Public License v3.0. See
[LICENSE](LICENSE).
