"""Compatibility shim backed by `mogumogu_advisories.report_html`."""
from pathlib import Path
import sys

_LIB_SRC = str(Path(__file__).resolve().parents[1] / "packages" / "mogumogu-advisories" / "src")
if _LIB_SRC not in sys.path:
    sys.path.insert(0, _LIB_SRC)

from mogumogu_advisories import report_html as _report_html  # noqa: E402

sys.modules[__name__] = _report_html
