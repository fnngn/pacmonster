# Pacmonster

Pacmonster is a lightweight Linux and Proxmox update orchestrator.

It discovers systems, detects supported package managers and executes updates from a single command while showing native package manager output, progress tracking and execution times.

Designed for homelabs and small infrastructure environments.

No agents.
No web UI.
No daemon.
No database.

Just:

```bash
pacmonster
```

---

## Mission

Pacmonster aims to make routine Linux maintenance distribution-agnostic.

Different Linux systems expose different package managers, but update management, status tracking and maintenance workflows share many common requirements.

Pacmonster provides a unified operational interface while continuing to use the native tooling of each distribution.

The project is intentionally not a replacement for package managers.

Instead, it serves as a communication and orchestration layer that translates common maintenance tasks into distribution-specific actions.

The long-term vision is a lightweight platform capable of maintaining small Linux landscapes through a consistent workflow, regardless of the underlying operating system.

---

## Features

- Automatic inventory discovery
- Multi-distribution support
- Package manager abstraction
- Native package manager output
- Progress tracking
- Runtime measurements
- Update history
- Update locking
- Interactive update confirmation
- Cron-friendly exit codes
- Inventory caching
- Status caching
- Doctor diagnostics
- JSON output support
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
| Debian | apt |
| Fedora | dnf |

### Virtual Machines

Current status:

- VM discovery available
- VM update support not yet implemented
- VM status support not yet implemented
- QEMU Guest Agent integration planned

---

## Installation

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

## Commands

### Update

Interactive update run:

```bash
pacmonster update
```

Unattended update run:

```bash
pacmonster update --yes
```

### Inventory

```bash
pacmonster inventory
```

### Status

```bash
pacmonster status
```

Verbose:

```bash
pacmonster status --verbose
```

### Refresh

```bash
pacmonster refresh
pacmonster refresh-status
```

### Dry Run

```bash
pacmonster dry-run
```

### History

```bash
pacmonster history
```

### Doctor

```bash
pacmonster doctor
```

---

## JSON Output

Pacmonster supports machine-readable JSON output.

```bash
pacmonster inventory --json
pacmonster status --json
pacmonster doctor --json
pacmonster dry-run --json
pacmonster history --json
```

---

## Update Workflow

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

## Exit Codes

| Code | Meaning |
|------|---------|
| 0 | Success |
| 1 | Update failures detected |
| 2 | User aborted operation |
| 3 | Update lock active |
| 4 | Runtime error |

---

## Cache Files

Pacmonster stores data in:

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
- Linux system maintenance

It is intentionally not intended to replace:

- Ansible
- Puppet
- Chef
- SaltStack
- Kubernetes

The goal is operational simplicity and distribution-independent maintenance workflows.

---

## Roadmap

- VM package detection
- VM update support
- VM status support
- QEMU Guest Agent integration
- Additional package managers
- Parallel execution
- Reboot tracking
- Maintenance windows

---

## Development

Run directly from the repository:

```bash
python3 -m pacmonster.main
```

Run syntax checks:

```bash
python3 -m py_compile pacmonster/*.py
```

---

## License

GNU General Public License v3.0

See the LICENSE file for details.
