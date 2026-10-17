# Pacmonster

Pacmonster is a lightweight Linux and Proxmox update orchestrator.

It discovers systems, detects supported package managers and executes updates from a single command while showing native package manager output, progress tracking and execution times.

Designed for homelabs.

No agents.
No web UI.
No daemon.
No database.

Just:

```bash
pacmonster
```

---

## Features

- Automatic inventory discovery
- Multi-distribution support
- Native package manager output
- Progress tracking
- Runtime measurements
- Update history
- Update locking
- Inventory caching
- Status caching
- Doctor diagnostics
- Installable Python package
- Proxmox-aware discovery

---

## Supported Platforms

### Hosts

| Distribution | Package Manager |
|-------------|----------------|
| Debian | apt |
| Proxmox VE | apt |
| Arch Linux | pacman |
| Fedora | dnf |

### Containers

| Distribution | Package Manager |
|-------------|----------------|
| Arch Linux | pacman |
| Debian* | apt |
| Fedora* | dnf |

\* Support depends on container discovery and package manager detection.

---

## Installation

Clone the repository:

```bash
git clone https://github.com/fnngn/pacmonster.git

cd pacmonster
```

Install in editable mode:

```bash
python3 -m pip install --break-system-packages -e .
```

Verify installation:

```bash
pacmonster help
```

---

## Quick Start

Run updates:

```bash
pacmonster
```

Example output:

```text
PACMONSTER UPDATE

STEP  TYPE  ID    NAME           DISTRO   PKG     PROGRESS
────  ────  ────  ─────────────  ──────   ──────  ──────────

1/9    HOST  host  pve            Debian    apt      █░░░░░░░░░ 11%
...

9/9    LXC   199   arch-template  Arch      pacman   ██████████ 100%

────────────────────────────────────────────────────────────

UPDATED      9
FAILED       0
DURATION     23.6s

────────────────────────────────────────────────────────────
```

---

## Commands

### Update

Run updates for all supported systems:

```bash
pacmonster
```

or

```bash
pacmonster update
```

---

### Inventory

Show discovered systems:

```bash
pacmonster inventory
```

---

### Status

Show update status:

```bash
pacmonster status
```

Verbose output:

```bash
pacmonster status --verbose
```

---

### Refresh

Refresh inventory information:

```bash
pacmonster refresh
```

Refresh status information:

```bash
pacmonster refresh-status
```

---

### Dry Run

Display planned update actions:

```bash
pacmonster dry-run
```

---

### History

Show recorded events:

```bash
pacmonster history
```

Example:

```text
2026-10-17T15:01:26 update
  updated: 9
  failed: 0
  duration: 8.1
```

---

### Doctor

Run diagnostics:

```bash
pacmonster doctor
```

---

## Update Workflow

Pacmonster follows a simple workflow:

```text
Inventory
    ↓
Package Manager Detection
    ↓
Build Action Plan
    ↓
Execute Updates
    ↓
Record History
```

Updates are executed sequentially.

Each system reports:

- progress
- package manager output
- execution time
- success or failure state

---

## Package Manager Support

### APT

```bash
apt-get update && apt-get upgrade -y
```

### Pacman

```bash
pacman -Syu --noconfirm
```

### DNF

```bash
dnf upgrade -y
```

---

## History

Pacmonster records:

- update runs
- status operations
- refresh operations

History is stored locally in:

```text
~/.cache/pacmonster/history.json
```

---

## Cache Files

Pacmonster stores cache data in:

```text
~/.cache/pacmonster/
```

Typical files:

```text
history.json
inventory.json
status.json
```

---

## Design Goals

Pacmonster was created for:

- homelabs
- personal infrastructure
- small Proxmox environments
- Linux administration

It is intentionally not intended to replace:

- Ansible
- Puppet
- Chef
- SaltStack
- Kubernetes

The goal is simplicity.

---

## Roadmap

Potential future improvements:

- VM package detection
- Additional package managers
- Parallel execution
- Update notifications
- Reboot tracking
- Maintenance windows

---

## Development

Run directly from the repository:

```bash
python -m pacmonster.main
```

Install editable:

```bash
python3 -m pip install --break-system-packages -e .
```

Run syntax checks:

```bash
python3 -m py_compile pacmonster/*.py
```

---

## License

GNU General Public License v3.0

See the LICENSE file for details.

