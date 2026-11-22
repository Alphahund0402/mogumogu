#!/usr/bin/env python3
"""Compatibility entry point backed by `packages/mogumogu-advisories`."""
from pathlib import Path
import sys

_LIB_SRC = str(Path(__file__).resolve().parents[1] / "packages" / "mogumogu-advisories" / "src")
if _LIB_SRC not in sys.path:
    sys.path.insert(0, _LIB_SRC)

from mogumogu_advisories import watcher as _watcher  # noqa: E402

if __name__ == "__main__":
    try:
        sys.exit(_watcher.main())
    except KeyboardInterrupt:
        sys.exit(130)
else:
    # Expose the underlying module object directly so `patch.object(watcher, ...)`
    # in existing tests continues to target the live module bindings.
    sys.modules[__name__] = _watcher
