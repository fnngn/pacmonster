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

Current release: **0.9.4** (CLI and TUI).

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
sudo pacmonster dry-run
sudo pacmonster history
sudo pacmonster doctor
sudo pacmonster witchdoctor
```

`dry-run` prints the planned update commands without executing them.
`witchdoctor` opens Doctor diagnostics in the interactive TUI.

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

Pacmonster executes updates through GhostPool, a worker pool with a maximum of four concurrent targets. Each Ghost is one execution slot. Targets are dispatched in FIFO order, and completed slots are reused until the queue is empty.

Package-manager output is shown live. For quiet operations, Pacmonster prints a running-time heartbeat every 10 seconds.

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
  "idle": 2,
  "capacity": 4,
  "ghosts": [
    {
      "id": 1,
      "busy": true,
      "target": "pihole"
    },
    {
      "id": 4,
      "busy": true,
      "target": "arch-template"
    }
  ]
}
```

---

# Dashboard

A lightweight Rust TUI provides live scheduler visibility and interactive
Doctor diagnostics. Its text panes support `↑`/`↓`, `PgUp`/`PgDn`, and `Home`/
`End`; the update view follows new output until you scroll away, and `End`
resumes following.

Current dashboard:

```text
Ghost Pool

ᗣ Active      2
◌ Idle        2
◌ Available   2

ᗣ G01 pihole
ᗣ G04 arch-template
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
Witchdoctor displays Doctor checks with colored PASS, WARN, SKIP, and FAIL
statuses, then asks whether to exit when the diagnostic run completes.

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
```

---

# License

GNU General Public License v3.0

See LICENSE for details.
