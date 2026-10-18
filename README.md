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

- #mission
- #current-status
- #architecture
- #features
- #supported-platforms
- #installation
- #commands
- #ghostpool-scheduler
- #runtime-state
- #dashboard
- #update-workflow
- #package-managers
- #exit-codes
- #cache-files
- #design-goals
- #roadmap
- #development
- #license

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
python3 -m pip install --break-system-packages -e .
```

Verify installation:

```bash
pacmonster help
```

---

# Commands

## Update

Interactive:

```bash
pacmonster update
```

Unattended:

```bash
pacmonster update --yes
```

## Inventory

```bash
pacmonster inventory
```

## Status

```bash
pacmonster status
```

Verbose:

```bash
pacmonster status --verbose
```

## Refresh

```bash
pacmonster refresh
pacmonster refresh-status
```

## Dry Run

```bash
pacmonster dry-run
```

## History

```bash
pacmonster history
```

## Doctor

```bash
pacmonster doctor
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

## Event Stream

```text
/var/tmp/pacmonster-events.jsonl
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
/var/tmp/pacmonster-state.json
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

# Development

Run directly:

```bash
python3 -m pacmonster.main
```

Python syntax checks:

```bash
python3 -m py_compile pacmonster/*.py
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
