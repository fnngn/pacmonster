import json
from pathlib import Path

EVENT_LOG = Path("/var/tmp/pacmonster-events.jsonl")
STATE_FILE = Path("/var/tmp/pacmonster-state.json")



def write_event(event: dict):
    """Write an event to JSONL."""

    with EVENT_LOG.open("a") as handle:
        handle.write(
            json.dumps(event) + "\n"
        )


def emit_ghost_event(event: str, ghost, target=None):
    """Emit a ghost lifecycle event."""

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

def write_state(pool):
    """Write current pool state."""

    with STATE_FILE.open("w") as handle:
        json.dump(
            pool.snapshot(),
            handle,
            indent=2
        )



def emit_pool_event(pool):
    """Emit current pool state."""

    snapshot = pool.snapshot()

    write_state(pool)


    write_event({
        "event": "pool",
        "active": snapshot["active"],
        "idle": snapshot["idle"],
        "capacity": snapshot["capacity"],
    })

    print(
        f"[POOL] active={snapshot['active']} idle={snapshot['idle']}"
    )
