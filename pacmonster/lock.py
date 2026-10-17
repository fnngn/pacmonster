from pathlib import Path


LOCK_DIR = Path.home() / ".cache" / "pacmonster"
LOCK_FILE = LOCK_DIR / "update.lock"


def is_locked():
    return LOCK_FILE.exists()


def acquire_lock():

    LOCK_DIR.mkdir(parents=True, exist_ok=True)

    if LOCK_FILE.exists():
        return False

    LOCK_FILE.write_text("locked")

    return True


def release_lock():

    if LOCK_FILE.exists():
        LOCK_FILE.unlink()
