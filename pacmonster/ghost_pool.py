from dataclasses import dataclass


@dataclass
class Ghost:
    """Represents one execution slot."""

    id: int
    busy: bool = False
    target: str | None = None
    progress: int | None = None
    elapsed_seconds: float = 0
    estimated_seconds: float | None = None


class GhostPool:
    """Manage ghost allocation and release."""

    def __init__(self, max_ghosts: int, cpu_cores: float | None = None):
        """Create a pool with the given number of initially idle slots."""

        self.max_ghosts = max_ghosts
        self.current_limit = max_ghosts
        self.cpu_cores = cpu_cores

        self.ghosts = [
            Ghost(id=index + 1)
            for index in range(max_ghosts)
        ]

    @property
    def active(self) -> int:
        """Return the number of currently assigned ghosts."""

        return sum(
            ghost.busy
            for ghost in self.ghosts
        )

    @property
    def idle(self) -> int:
        """Return the number of currently unassigned ghosts."""

        return max(0, self.current_limit - self.active)

    @property
    def available(self) -> int:
        """Return the number of ghosts available for another target."""

        return max(0, self.current_limit - self.active)

    def has_capacity(self) -> bool:
        """Return whether at least one ghost can be allocated."""

        return self.available > 0

    def set_limit(self, limit: int) -> None:
        """Adjust available slots without interrupting already-running work."""

        self.current_limit = min(self.max_ghosts, max(1, limit))

    def allocate(
        self, target: str, estimated_seconds: float | None = None
    ) -> Ghost | None:
        """Assign a free ghost to target, or return None when the pool is full."""

        if not self.has_capacity():
            return None

        for ghost in self.ghosts:

            if not ghost.busy:

                ghost.busy = True
                ghost.target = target
                ghost.progress = 0 if estimated_seconds else None
                ghost.elapsed_seconds = 0
                ghost.estimated_seconds = estimated_seconds

                return ghost

        return None

    def release(self, ghost_id: int) -> None:
        """Mark the identified ghost idle and clear its assigned target."""

        for ghost in self.ghosts:

            if ghost.id == ghost_id:

                ghost.busy = False
                ghost.target = None
                ghost.progress = None
                ghost.elapsed_seconds = 0
                ghost.estimated_seconds = None

                return

    def update_progress(self, ghost_id: int, elapsed_seconds: float) -> None:
        """Record elapsed time and a capped estimate for an active update."""

        for ghost in self.ghosts:
            if ghost.id == ghost_id and ghost.busy:
                ghost.elapsed_seconds = elapsed_seconds
                if ghost.estimated_seconds:
                    ghost.progress = min(
                        95,
                        int(elapsed_seconds / ghost.estimated_seconds * 100),
                    )
                return

    def snapshot(self) -> dict:
        """Return a serializable snapshot of capacity and ghost assignments."""

        active = self.active

        return {
            "active": active,
            "idle": self.max_ghosts - active,
            "capacity": self.current_limit,
            "cpu_cores": self.cpu_cores,
            "ghosts": [
                {
                    "id": ghost.id,
                    "busy": ghost.busy,
                    "target": ghost.target,
                    "progress": ghost.progress,
                    "elapsed_seconds": round(ghost.elapsed_seconds, 1),
                    "estimated_seconds": ghost.estimated_seconds,
                }
                for ghost in self.ghosts
            ]
        }
