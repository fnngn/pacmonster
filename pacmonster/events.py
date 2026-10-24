import json
import os
from pathlib import Path

RUNTIME_DIR = Path("/var/tmp")
EVENT_LOG = RUNTIME_DIR / f"pacmonster-events-{os.geteuid()}.jsonl"
STATE_FILE = RUNTIME_DIR / f"pacmonster-state-{os.geteuid()}.json"



def write_event(event: dict):
    """Append one event as a JSON line to the current user's runtime log."""

    with EVENT_LOG.open("a") as handle:
        handle.write(
            json.dumps(event) + "\n"
        )


def emit_ghost_event(event: str, ghost, target=None):
    """Log and print a ghost assignment or release event."""

    write_event({
        "event": event,
        "ghost": ghost.id,
        "target": target,
    })

    if event == "assigned":

        print(
            f"[G{ghost.id:02}] assigned -> {target}"
        )

    elif event == "released":

        print(
            f"[G{ghost.id:02}] released <- {target}"
        )

def write_state(snapshot: dict):
    """Persist a serializable GhostPool snapshot to the runtime state file."""

    with STATE_FILE.open("w") as handle:
        json.dump(
            snapshot,
            handle,
            indent=2
        )



def emit_pool_event(pool):
    """Persist and log the pool snapshot, then print its active and idle counts."""

    snapshot = pool.snapshot()
    write_state(snapshot)

    write_event({
        "event": "pool",
        "active": snapshot["active"],
        "idle": snapshot["idle"],
        "capacity": snapshot["capacity"],
    })

    print(
        f"[POOL] active={snapshot['active']} idle={snapshot['idle']}"
    )
