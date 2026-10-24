from dataclasses import dataclass


@dataclass
class Ghost:
    """Represents one execution slot."""

    id: int
    busy: bool = False
    target: str | None = None


class GhostPool:
    """Manage ghost allocation and release."""

    def __init__(self, max_ghosts: int):
        """Create a pool with the given number of initially idle slots."""

        self.max_ghosts = max_ghosts

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

        return self.max_ghosts - self.active

    @property
    def available(self) -> int:
        """Return the number of ghosts available for another target."""

        return sum(
            not ghost.busy
            for ghost in self.ghosts
        )

    def has_capacity(self) -> bool:
        """Return whether at least one ghost can be allocated."""

        return self.available > 0

    def allocate(self, target: str) -> Ghost | None:
        """Assign a free ghost to target, or return None when the pool is full."""

        for ghost in self.ghosts:

            if not ghost.busy:

                ghost.busy = True
                ghost.target = target

                return ghost

        return None

    def release(self, ghost_id: int) -> None:
        """Mark the identified ghost idle and clear its assigned target."""

        for ghost in self.ghosts:

            if ghost.id == ghost_id:

                ghost.busy = False
                ghost.target = None

                return

    def snapshot(self) -> dict:
        """Return a serializable snapshot of capacity and ghost assignments."""

        active = self.active

        return {
            "active": active,
            "idle": self.max_ghosts - active,
            "capacity": self.max_ghosts,
            "ghosts": [
                {
                    "id": ghost.id,
                    "busy": ghost.busy,
                    "target": ghost.target,
                }
                for ghost in self.ghosts
            ]
        }
