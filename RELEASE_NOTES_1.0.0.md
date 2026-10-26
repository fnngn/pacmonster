# Pacmonster 1.0.0

Pacmonster 1.0 establishes the project as a task-focused Linux and Proxmox
update orchestrator with a consistent terminal interface, command-line
operations, and machine-readable output.

## What is included

- Guided TUIs for update control, system and installation checks, inventory and
  snapshots, log search, recovery, and project help.
- Matching CLI entry points for operational workflows, keeping TUI use
  optional for scripts and direct administration.
- Versioned JSON update reports with system identity, distribution, package
  manager, kernel when available, result status, and package version changes.
- Snapshot-backed package recovery and safer update scheduling for supported
  Linux hosts and Proxmox LXC containers.
- Optional verified prebuilt TUI binaries; the default installer builds from
  source and caches Cargo artifacts between installations.

## Architecture

Pacmonster began as a Python-only project and is evolving into a hybrid
Python/Rust application. Python currently coordinates package managers and
system operations; Rust powers the increasingly central TUI. This release does
not represent a completed rewrite of the operational core. The CLI remains a
first-class interface, and JSON remains the integration foundation.

## Scope and limitations

- Supported update targets are Linux hosts and supported Proxmox LXC
  containers using APT, pacman, or DNF.
- Proxmox VM discovery is available, but VM status and update operations are
  not implemented.
- Docker and Compose lifecycle updates are not implemented.
- Recovery restores package versions only when the exact package archives are
  available; it is not a filesystem, configuration, or user-data backup.

## Project direction

The TUI will continue to grow as the guided operational experience without
making the CLI or automation paths second-class. Planned work includes a
stronger troubleshooting workflow in Witchdoctor, verified parity between TUI
and CLI operations, and stable documented JSON schemas.

Docker support is a possible future area, but safe updates should follow the
container lifecycle: pull an image and recreate a container or Compose service,
preserve explicitly declared state, validate health, and provide a rollback
path. It should not mutate packages inside arbitrary running containers.
Discovery, image selection, stateful volumes, downtime, and rollback behavior
must be designed and tested before this is supported.

Further plans include broader VM/platform support and selective migration of
core components to Rust where it improves reliability or maintainability.
These are future directions, not features promised by 1.0.0.
