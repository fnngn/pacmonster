"""Search Pacmonster and native package-manager logs."""

import json
import re
from collections import deque
from pathlib import Path

from .events import EVENT_LOG

MAX_LINES_PER_LOG = 5000
LOG_PATHS = (
    Path("/var/log/apt/history.log"),
    Path("/var/log/apt/term.log"),
    Path("/var/log/dpkg.log"),
    Path("/var/log/pacman.log"),
    Path("/var/log/dnf.log"),
    Path("/var/log/yum.log"),
)
ISSUE_PATTERN = re.compile(
    r"^\s*[EW]:|(?<![\w-])(error|failed|failure|fatal|warning|warn|denied|timeout|"
    r"no space left|corrupt|broken|cannot|could not)(?![\w-])",
    re.IGNORECASE,
)
ERROR_PATTERN = re.compile(
    r"^\s*E:|(?<![\w-])(error|failed|failure|fatal|denied|no space left|corrupt|broken|"
    r"cannot|could not)(?![\w-])",
    re.IGNORECASE,
)


def _record(source, message, timestamp="", level=None):
    """Normalize one raw log line into a searchable record."""

    detected_level = level or (
        "error" if ERROR_PATTERN.search(message)
        else "warning" if ISSUE_PATTERN.search(message)
        else "info"
    )
    return {
        "timestamp": timestamp,
        "level": detected_level,
        "source": str(source),
        "message": message,
    }


def _read_event_log(path):
    if not path.is_file():
        return []
    records = []
    try:
        with path.open(errors="replace") as log:
            lines = list(deque(log, maxlen=MAX_LINES_PER_LOG))
    except OSError as error:
        return [_record(path, f"Could not read log: {error}", level="error")]
    for line in lines:
        line = line.rstrip("\n")
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            records.append(_record(path, f"Malformed event record: {line}", level="warning"))
            continue
        records.append(
            _record(
                path,
                json.dumps(event, sort_keys=True),
                timestamp=str(event.get("timestamp", "")),
                level=(
                    "error"
                    if event.get("event") in {"error", "failed", "failure"}
                    else None
                ),
            )
        )
    return records


def _read_text_log(path):
    if not path.is_file():
        return []
    try:
        with path.open(errors="replace") as log:
            lines = list(deque(log, maxlen=MAX_LINES_PER_LOG))
    except OSError as error:
        return [_record(path, f"Could not read log: {error}", level="error")]
    return [_record(path, line.rstrip("\n")) for line in lines]


def search_logs(query=None, issues_only=False, limit=2000):
    """Search available Pacmonster and package-manager logs."""

    records = _read_event_log(EVENT_LOG)
    for path in LOG_PATHS:
        records.extend(_read_text_log(path))

    needle = query.casefold() if query else None
    if issues_only:
        records = [record for record in records if record["level"] in {"error", "warning"}]
    if needle:
        records = [
            record
            for record in records
            if needle in record["message"].casefold()
            or needle in record["source"].casefold()
        ]
    return records[-max(1, min(limit, 10000)):]
