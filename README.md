# Pacmonster

Pacmonster is a lightweight Linux and Proxmox update orchestrator designed for homelabs and small infrastructure environments.

It discovers systems, identifies supported package managers and executes updates through a parallel worker scheduler while preserving native package manager behavior.

No agents.

No web UI.

No daemon.

No database.

Just:

```bash
pacmonster update
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

Pacmonster provides a consistent operational interface for maintaining heterogeneous Linux environments.

The project does not replace package managers.

Instead it acts as an orchestration layer that translates maintenance actions into distribution-specific package manager operations.

The long-term goal is a lightweight platform capable of managing small Linux landscapes through a unified workflow regardless of distribution.

---

# Current Status

Version:

```text
v0.9
```

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

Current status:

- VM discovery available
- VM updates not implemented
- VM status not implemented

---

# Installation

Clone repository:

```bash
git clone https://github.com/fnngn/pacmonster.git
cd pacmonster
```

Install:

```bash
./install.sh
```

Verify installation:

```bash
sudo pacmonster help
```

The installer uses `sudo` when needed and installs the command under `/usr/local/bin`, so it is available to `sudo` without depending on the user's `~/.local/bin`.

---

# Commands

## Update

Interactive:

```bash
sudo pacmonster update
```

Unattended:

```bash
sudo pacmonster update --yes
```

## Inventory

```bash
sudo pacmonster refresh
sudo pacmonster inventory
```

## Status

```bash
sudo pacmonster refresh-status
sudo pacmonster status
```

Verbose:

```bash
sudo pacmonster status --verbose
```

`inventory` and `status` display cached snapshots. Run the corresponding refresh command to discover systems or check for current package updates.

## Dry Run

```bash
sudo pacmonster dry-run
```

## History

```bash
sudo pacmonster history
```

## Doctor

```bash
sudo pacmonster doctor
```

---

# GhostPool Scheduler

Pacmonster executes updates through a fixed-size worker pool called GhostPool.

Each Ghost represents one concurrent execution slot.

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

## Event Stream

```text
/var/tmp/pacmonster-events-<uid>.jsonl
```

Append-only scheduler event log.

Example:

```json
{
  "event": "assigned",
  "ghost": 1,
  "target": "pve"
}
```

## Scheduler State

```text
/var/tmp/pacmonster-state-<uid>.json
```

Current runtime snapshot.

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

A lightweight Rust TUI provides live scheduler visibility.

Current dashboard:

```text
Ghost Pool

ᗣ Active      2
◌ Idle        2
◌ Available   2

ᗣ G01 pihole
ᗣ G04 arch-template
```

Launch:

```bash
cd tui
cargo run
```

Quit:

```text
q
```

---

# Update Workflow

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

This maintenance work was completed with assistance from GitHub Copilot.

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

---

# License

GNU General Public License v3.0

See LICENSE for details.
