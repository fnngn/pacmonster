# Pacmonster

Pacmonster is a lightweight Linux and Proxmox update orchestrator designed for homelabs and small infrastructure environments.

It discovers systems, identifies supported package managers and executes updates through a parallel worker scheduler while preserving native package manager behavior.

No agents.

No web UI.

No daemon.

No database.

Just:

```bash
sudo pacmonster
```

---

# Table of Contents

- [Mission](#mission)
- [Current Status](#current-status)
- [Architecture](#architecture)
- [Features](#features)
- [Supported Platforms](#supported-platforms)
- [Installation](#installation)
- [Commands](#commands)
- [Terminal Interfaces](#terminal-interfaces)
- [GhostPool Scheduler](#ghostpool-scheduler)
- [Runtime State](#runtime-state)
- [Dashboard](#dashboard)
- [Update Workflow](#update-workflow)
- [Package Managers](#package-managers)
- [Exit Codes](#exit-codes)
- [Cache Files](#cache-files)
- [Design Goals](#design-goals)
- [Roadmap](#roadmap)
- [Development](#development)
- [Credits](#credits)
- [License](#license)

---

# Mission

Pacmonster provides a consistent operational interface for maintaining heterogeneous Linux environments. It orchestrates native package managers rather than replacing them.

---

# Current Status

Current release: **0.9.5** (CLI and TUI).

Implemented:

- Automatic inventory discovery
- Multi-distribution support
- Package manager abstraction
- GhostPool scheduler
- Parallel update execution
- Runtime state snapshots
- JSON event streams
- Rust terminal dashboard
- Active worker tracking
- Update history
- Status caching
- Doctor diagnostics
- Package-state snapshots and cached-archive recovery
- Proxmox-aware discovery

---

# Architecture

```text
pacmonster
    │
    ▼
Inventory
    │
    ▼
Update Planner
    │
    ▼
GhostPool Scheduler
    │
    ├── Update Workers
    │
    ├── JSON Event Stream
    │
    └── Runtime State
            │
            ▼
        Rust Dashboard
```

---

# Features

- Parallel update execution
- Ghost worker scheduler
- Native package manager output
- Runtime measurements
- Execution history
- Runtime state snapshots
- JSON output support
- Inventory caching
- Status caching
- Interactive updates
- Unattended updates
- Doctor diagnostics
- Interactive Doctor diagnostics (`witchdoctor`)
- Pacmonster installation checks
- Archeologist log search and warning/error filtering
- Start menu and Advisor help/handbook interface
- Proxmox-aware discovery
- Cron-friendly exit codes

---

# Supported Platforms

## Hosts

| Distribution | Manager |
|--------------|----------|
| Debian | apt |
| Proxmox VE | apt |
| Arch Linux | pacman |
| Fedora | dnf |

## Containers

| Distribution | Manager |
|--------------|----------|
| Debian | apt |
| Arch Linux | pacman |
| Fedora | dnf |

## Virtual Machines

VM discovery is available; VM status checks and updates are not implemented.

---

# Installation

Clone repository:

```bash
git clone https://github.com/fnngn/pacmonster.git
cd pacmonster
```

Install the system-wide CLI (the installer requests `sudo` if needed):

```bash
./install.sh
```

The installer checks for a native C compiler, installs the platform build
tools if needed, ensures Rust/Cargo 1.85 or newer is available, builds the
TUI, installs `pacmonster-tui`, and prepares the root-owned package snapshot
store. If a suitable system Rust toolchain is unavailable, it bootstraps the
official stable toolchain through rustup.

Verify the installation:

```bash
sudo pacmonster help
```

The installer places the command in `/usr/local/bin`, so it is available as `sudo pacmonster` without relying on a user's `~/.local/bin`. Re-run the installer after changing the source code to update the installed copy.

Pacmonster needs root privileges for host package updates and Proxmox container discovery. It also needs the relevant native package manager and, for Proxmox discovery, `pct` and `qm`.

---

# Commands

## Update systems

Running Pacmonster without a subcommand starts an interactive update. Updates are real package-manager operations.

Interactive update:

```bash
sudo pacmonster
```

Equivalent explicit command:

```bash
sudo pacmonster update
```

Unattended update (does not prompt for confirmation):

```bash
sudo pacmonster update --yes
```

## Inventory and status

```bash
sudo pacmonster refresh
sudo pacmonster inventory
sudo pacmonster refresh-status
sudo pacmonster status
```

`inventory` and `status` display cached snapshots. Run `refresh` or `refresh-status` to update those caches. Add `--json` to either display command for JSON output; status also supports:

```bash
sudo pacmonster status --verbose
```

## Other commands

```bash
sudo pacmonster start
sudo pacmonster ghosthunter
sudo pacmonster witchdoctor
pacmonster librarian
pacmonster archeologist
pacmonster advisor
pacmonster help
sudo pacmonster dry-run
sudo pacmonster history
sudo pacmonster doctor
sudo pacmonster witchdoctor
sudo pacmonster doctor --installation
sudo pacmonster archeologist
sudo pacmonster archeologist issues
sudo pacmonster archeologist search "disk space"
```

`pacmonster start` opens the main menu for Ghosthunter, Witchdoctor,
Librarian, Archeologist, Advisor, and Restaurator. Restaurator currently opens
its snapshot list; use `sudo pacmonster restaurator restore SNAPSHOT_ID` to
restore a selected snapshot.
`dry-run` prints the planned update commands without executing them.
`witchdoctor` opens an old-library-themed diagnostic menu with system and
Pacmonster installation checks. `doctor --installation` runs the installation
checks directly. `archeologist` opens a log browser; its `issues` view reduces
records to likely warnings and errors, while `search QUERY` finds matching
text. The CLI also accepts `archeologist all|issues|search QUERY --json`.
`pacmonster advisor` opens the reference-room interface. The plain
`pacmonster help` command displays available commands; `pacmonster help --json`
returns them as JSON for integrations.

## Terminal Interfaces

Pacmonster's terminal interfaces are task-oriented front ends for the CLI:

| Interface | Command | Purpose |
|-----------|---------|---------|
| Start | `pacmonster start` | Main menu for all interfaces |
| Ghosthunter | `sudo pacmonster ghosthunter` | Run planned updates and monitor workers |
| Witchdoctor | `sudo pacmonster witchdoctor` | Diagnose the host and check the installation |
| Librarian | `pacmonster librarian` | Browse and create package recovery snapshots |
| Archeologist | `pacmonster archeologist` | Search logs and filter likely warnings/errors |
| Advisor | `pacmonster advisor` | Browse CLI help, README, license, and developer information |

The interfaces share a parchment, petrol, and gold palette. In scrollable
views, use the arrow keys or `PgUp`/`PgDn`; `Home` and `End` jump to the
beginning and end, while `Esc` returns to the previous menu. The Advisor loads
its handbook from `README.md` and its license text from `LICENSE` (both
embedded during compilation, with runtime file overrides for checkout or
packaged copies). Command help comes from the CLI's JSON output. Thus the
in-app handbook follows the README instead of maintaining a separate copy.

## Package snapshots and recovery

Before starting updates, Pacmonster's Librarian saves each target's installed
package/version manifest and only the matching package archives needed for
recovery. Archives are content-addressed and shared between snapshots: unchanged
package files are stored once, while newly changed package files add only their
own size. Identical archives from different LXC containers (or the host) also
share a blob, but only after their content hashes match; matching package names
and versions alone are not treated as proof that the files are identical. When
a retained blob already matches an LXC package, Pacmonster verifies its hash
inside the container and avoids copying the archive to the host again. The
three newest manifests per target are kept under
`/var/lib/pacmonster/snapshots/`; blobs no longer referenced by a retained
snapshot are removed.
Doctor also runs the host package manager's native consistency check.
Snapshots require enough free disk space; check `sudo pacmonster doctor` for
storage and archive availability.
Long-running snapshot creation reports package and archive progress. Press
`Ctrl+C` to cancel; incomplete snapshot metadata and temporary package files are
cleaned up, and Pacmonster exits with status 130.

```bash
sudo pacmonster librarian list
sudo pacmonster librarian snapshot [TARGET_ID]
sudo pacmonster librarian show SNAPSHOT_ID
sudo pacmonster restaurator list
sudo pacmonster restaurator restore SNAPSHOT_ID
```

Run `sudo pacmonster librarian` to enter the Librarian's old-library-inspired
interface. Browse the catalogue, create new volumes of package snapshots, and
inspect their records. The explicit `librarian list|snapshot|show` commands
remain available for scripts and direct CLI use.

Restoration compares installed versions with the selected snapshot and uses
matching package archives from that snapshot. It previews the changes and
prompts before changing packages; `--yes` skips that confirmation. If an
exact-version archive is missing, restoration stops without installing a
partial set. A manifest-only snapshot can help diagnose versions but cannot
restore packages without cached archives or repository access. This restores
package versions only; packages added after the snapshot are left installed.
This is not a filesystem, configuration, or data backup.

---

# GhostPool Scheduler

Pacmonster executes updates through GhostPool. Each Ghost is one execution
slot; targets are dispatched in FIFO order, and completed slots are reused
until the queue is empty. Worker count is based on the CPUs available to the
process (including CPU affinity and detected cgroup quotas), with at most half
of those cores used (with a minimum of one worker when work exists) and an
overall cap of eight workers. The limit is
re-evaluated while scheduling: available memory and current system load can
reduce how many new targets start without interrupting work already underway.
At least one worker is kept when there is work to do.

When available, memory is measured against a 1 GiB reserve and a 1 GiB
allowance per worker. The update TUI displays progress estimates based on
recent successful runs for the same target. Until a target has history, its
progress bar is indeterminate rather than presenting a fabricated percentage.

Lifecycle:

```text
Idle
  │
  ▼
Assigned
  │
  ▼
Updating
  │
  ▼
Released
  │
  ▼
Idle
```

The scheduler continuously tracks:

- Active workers
- Idle workers
- Pool capacity
- Current worker assignments
- Per-target elapsed time and estimated progress

---

# Runtime State

Runtime information is exported under:

```text
/var/tmp/
```

Event and state files include the effective user ID in their names so root and regular-user runs do not conflict over file ownership.

Package snapshots are stored separately under `/var/lib/pacmonster/snapshots/`.

## Event stream

```text
/var/tmp/pacmonster-events-<uid>.jsonl
```

Append-only scheduler event log. `<uid>` is the effective user ID of the Pacmonster process.

Example:

```json
{
  "event": "assigned",
  "ghost": 1,
  "target": "pve"
}
```

## Scheduler state

```text
/var/tmp/pacmonster-state-<uid>.json
```

Current runtime snapshot, also separated by effective user ID.

Example:

```json
{
  "active": 2,
  "idle": 0,
  "capacity": 2,
  "cpu_cores": 4,
  "ghosts": [
    {
      "id": 1,
      "busy": true,
      "target": "pihole",
      "progress": 70,
      "elapsed_seconds": 42.0,
      "estimated_seconds": 60.0
    },
    {
      "id": 4,
      "busy": true,
      "target": "arch-template",
      "progress": null,
      "elapsed_seconds": 11.0,
      "estimated_seconds": null
    }
  ]
}
```

---

# Dashboard

A lightweight Rust TUI provides live scheduler visibility, the main menu, and
the Ghosthunter, Witchdoctor, Librarian, Archeologist, and Advisor
interfaces. Its text panes support `↑`/`↓`, `PgUp`/`PgDn`, and `Home`/`End`;
the update view follows new output until you scroll away, and `End` resumes
following.

Current dashboard:

```text
THE GHOST POOL

ᗣ Active      2
◌ Idle        0
◌ Available   0

ᗣ G01 [#######---] [~70%] pihole
ᗣ G02 [---###----] [estimating] arch-template
```

The TUI builds with Rust/Cargo **1.85 or newer**; Proxmox utilities are not
needed to compile or open the dashboard. Install a Rust toolchain if `cargo`
is not available, then build from the repository:

```bash
cd tui
cargo --version
cargo build
cargo run
```

`cargo run` opens the dashboard without starting updates. To test the full
update flow directly from the checkout, run:

```bash
cargo run -- --run-updates
```

When the `pacmonster` command is installed, the TUI uses it. Otherwise a TUI
built from this checkout runs the Python module from the repository source.
Install the optimized executable for the `ghosthunter` CLI command:

```bash
cargo build --release
sudo install -m 755 target/release/pacmonster-tui /usr/local/bin/pacmonster-tui
```

Start the update workflow inside the TUI:

```bash
sudo pacmonster ghosthunter
```

Open the Doctor diagnostics in the TUI:

```bash
sudo pacmonster witchdoctor
```

Ghosthunter starts `pacmonster update --yes`, so it does not stop at the
interactive `Continue update run?` confirmation. It streams the update output
into the dashboard and asks whether to exit after all targets finish. During a
run, `q` or `Ctrl+C` requests an interrupt and lets Pacmonster stop
package-manager processes safely. The update workflow itself requires a Linux
host with a supported package manager; LXC discovery and updates require
Proxmox's `pct` command.
Witchdoctor provides a menu for host diagnostics and Pacmonster installation
checks. The Archeologist can browse available Pacmonster and package-manager
logs, reduce them to likely warnings/errors, or search for a term. Both
interfaces use the standard parchment, petrol, and gold palette; result panes
can be scrolled with the arrow and page keys. The Librarian offers a snapshot
catalogue and creation flow. The Advisor shows CLI help, a readable version
of this README, the full license text, and project/build information.

---

# Update workflow

```text
Inventory
    ↓
Package Manager Detection
    ↓
Build Update Plan
    ↓
GhostPool Scheduler
    ↓
Parallel Updates
    ↓
State Snapshot
    ↓
Rust Dashboard
```

---

# Package Managers

## APT

```bash
apt-get update && apt-get upgrade -y
```

## Pacman

```bash
pacman -Syu --noconfirm
```

## DNF

```bash
dnf upgrade -y
```

---

# Exit Codes

| Code | Meaning |
|--------|----------|
| 0 | Success |
| 1 | Update failure |
| 2 | User aborted |
| 3 | Update lock active |
| 4 | Runtime error |
| 130 | Update interrupted with Ctrl+C |

When interrupted, Pacmonster sends `SIGINT` to active package-manager process groups. If they do not exit, it escalates to `SIGTERM` and then `SIGKILL`.

---

# Cache Files

Pacmonster stores local cache data under:

```text
~/.cache/pacmonster/
```

Examples:

```text
history.json
inventory.json
status.json
```

---

# Design Goals

Pacmonster is intended for:

- Homelabs
- Proxmox environments
- Personal infrastructure
- Small Linux environments
- Routine maintenance workflows

Pacmonster is intentionally not a replacement for:

- Ansible
- Puppet
- Chef
- SaltStack
- Kubernetes

The focus is operational simplicity.

---

# Roadmap

## v1.0

- Dashboard cleanup
- Better terminal restoration
- Failure visualization
- Retry support
- Runtime metrics
- Host completion tracking
- Stable dashboard state schema

## Future

- VM update support
- VM status support
- Reboot tracking
- Maintenance windows
- Additional package managers

---

# Credits

Maintenance changes were made with assistance from GitHub Copilot.

---

# Development

Run directly:

```bash
python3 -m pacmonster.main
```

Python syntax checks:

```bash
python3 -m py_compile pacmonster/*.py
```

Run tests:

```bash
python3 -m unittest discover -s tests
```

Dashboard:

```bash
cd tui
cargo build
cargo run
```

Run the update or Doctor workflow from the source checkout:

```bash
cargo run -- --run-updates
cargo run -- --run-doctor
cargo run -- --run-librarian
cargo run -- --run-archeologist
cargo run -- --run-advisor
```

---

# License

GNU General Public License v3.0

See LICENSE for details.
