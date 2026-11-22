"""Internal filesystem helpers shared across report writers."""
from pathlib import Path
import tempfile


def atomic_write_text(path: Path, content: str, *, prefix: str = "mogumogu-out-", filename: str = "output.tmp") -> None:
    """Writes UTF-8 text atomically via a temporary directory beside `path`."""
    dest = Path(path)
    dest.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.TemporaryDirectory(prefix=prefix, dir=dest.parent) as directory:
        pending = Path(directory) / filename
        pending.write_text(content, encoding="utf-8")
        pending.replace(dest)
