# Pacmonster

Pacmonster is a lightweight Proxmox update orchestrator for homelab environments.

It discovers updateable systems and executes updates through a single command.

```bash
pacmonster
```

## Features

- Proxmox inventory discovery
- Arch Linux LXC updates
- Debian / Proxmox host updates
- Native pacman output
- Native apt output
- Inventory-based updates
- Update history
- Update locking
- Status cache
- Doctor checks

## Installation

```bash
git clone 
cd pacmonster

python3 -m pip install --break-system-packages -e .
```

## Usage

Update everything:

```bash
pacmonster
```

Show status:

```bash
pacmonster status
```

Verbose status:

```bash
pacmonster status --verbose
```

Dry run:

```bash
pacmonster dry-run
```

Refresh status cache:

```bash
pacmonster refresh-status
```

History:

```bash
pacmonster history
```

Doctor:

```bash
pacmonster doctor
```

## Example

```text
╔══════════════════════════════════════╗
║         PACMONSTER UPDATE           ║
╚══════════════════════════════════════╝

━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
[1/9] host pve
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

...

✓ Completed in 0.8s
```

## License

GNU General Public License v3.0

See LICENSE for details.
