from dataclasses import dataclass


@dataclass
class SystemTarget:
    id: str
    name: str
    kind: str
    distro: str
    status: str
    package_manager: str = "unknown"
    can_check_updates: bool = False
    can_upgrade: bool = False
    updates: int = 0
