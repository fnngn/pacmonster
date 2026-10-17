from dataclasses import dataclass


@dataclass
class SystemTarget:
    kind: str
    id: str
    name: str
    distro: str
    update_count: int = 0
