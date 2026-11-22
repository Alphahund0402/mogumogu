"""Verify zero-dependency PEP 517 wheel packaging of mogumogu-advisories."""
import importlib.util
from pathlib import Path
import tempfile
import unittest
import zipfile

PKG_ROOT = Path(__file__).resolve().parents[1]


def load_backend():
    spec = importlib.util.spec_from_file_location("_build_backend", PKG_ROOT / "_build_backend.py")
    assert spec and spec.loader
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module


class PackagingTests(unittest.TestCase):
    def test_stdlib_backend_builds_valid_wheel_with_console_script(self):
        backend = load_backend()
        self.assertEqual(backend.get_requires_for_build_wheel(), [])
        with tempfile.TemporaryDirectory() as directory:
            wheel_name = backend.build_wheel(directory)
            wheel_path = Path(directory) / wheel_name
            self.assertTrue(wheel_path.is_file())
            with zipfile.ZipFile(wheel_path) as zf:
                names = set(zf.namelist())
                self.assertIn("mogumogu_advisories/__init__.py", names)
                self.assertIn("mogumogu_advisories/cli.py", names)
                self.assertIn("mogumogu_advisories/report_sarif.py", names)
                self.assertIn("mogumogu_advisories-0.1.0.dist-info/METADATA", names)
                self.assertIn("mogumogu_advisories-0.1.0.dist-info/WHEEL", names)
                self.assertIn("mogumogu_advisories-0.1.0.dist-info/RECORD", names)
                entry_points = zf.read("mogumogu_advisories-0.1.0.dist-info/entry_points.txt").decode("utf-8")
                self.assertIn("mogumogu-advisories = mogumogu_advisories.cli:main", entry_points)


if __name__ == "__main__":
    unittest.main()
