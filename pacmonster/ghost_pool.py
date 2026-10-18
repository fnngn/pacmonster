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

        self.max_ghosts = max_ghosts

        self.ghosts = [
            Ghost(id=index + 1)
            for index in range(max_ghosts)
        ]

    @property
    def active(self) -> int:
        """Return active ghost count."""

        return sum(
            ghost.busy
            for ghost in self.ghosts
        )

    @property
    def idle(self) -> int:
        """Return idle ghost count."""

        return self.max_ghosts - self.active

    @property
    def available(self) -> int:
        """Return available ghost count."""

        return sum(
            not ghost.busy
            for ghost in self.ghosts
        )

    def has_capacity(self) -> bool:
        """Return True if a ghost can be allocated."""

        return self.available > 0

    def allocate(self, target: str) -> Ghost | None:
        """Allocate a free ghost."""

        for ghost in self.ghosts:

            if not ghost.busy:

                ghost.busy = True
                ghost.target = target

                return ghost

        return None

    def release(self, ghost_id: int) -> None:
        """Release an allocated ghost."""

        for ghost in self.ghosts:

            if ghost.id == ghost_id:

                ghost.busy = False
                ghost.target = None

                return

    def snapshot(self) -> dict:
        """Return current pool state."""

        return {
            "active": self.active,
            "idle": self.idle,
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
