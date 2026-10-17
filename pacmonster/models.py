from dataclasses import dataclass


@dataclass
class SystemTarget:
    id: str
    name: str
    kind: str
    distro: str
    status: str
    updates: int = 0
