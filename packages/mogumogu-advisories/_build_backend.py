"""Zero-dependency PEP 517 / PEP 660 build backend using Python's standard library.

Enables fully offline `pip install` and `pip install -e` without downloading
external build tools.
"""
import base64
import hashlib
from pathlib import Path
import tomllib
import zipfile

ROOT = Path(__file__).resolve().parent
SRC_DIR = ROOT / "src"
PKG_NAME = "mogumogu_advisories"


def _meta() -> dict:
    return tomllib.loads((ROOT / "pyproject.toml").read_text(encoding="utf-8"))["project"]


def _dist_name(name: str) -> str:
    return name.replace("-", "_")


def _metadata_text(project: dict) -> str:
    lines = [
        "Metadata-Version: 2.1",
        f"Name: {project['name']}",
        f"Version: {project['version']}",
        f"Summary: {project['description']}",
        f"License: {project['license']['text']}",
        f"Requires-Python: {project['requires-python']}",
        "Description-Content-Type: text/markdown",
    ]
    for keyword in project.get("keywords", []):
        lines.append(f"Keywords: {keyword}")
    for classifier in project.get("classifiers", []):
        lines.append(f"Classifier: {classifier}")
    readme = ROOT / project.get("readme", "README.md")
    body = readme.read_text(encoding="utf-8") if readme.is_file() else ""
    return "\n".join(lines) + "\n\n" + body


def _entry_points_text(project: dict) -> str:
    scripts = project.get("scripts", {})
    if not scripts:
        return ""
    lines = ["[console_scripts]"]
    for name, target in scripts.items():
        lines.append(f"{name} = {target}")
    return "\n".join(lines) + "\n"


def _wheel_text() -> str:
    return (
        "Wheel-Version: 1.0\n"
        "Generator: mogumogu-stdlib-backend\n"
        "Root-Is-Purelib: true\n"
        "Tag: py3-none-any\n"
    )


def _record_line(arcname: str, data: bytes) -> str:
    digest = base64.urlsafe_b64encode(hashlib.sha256(data).digest()).rstrip(b"=").decode("ascii")
    return f"{arcname},sha256={digest},{len(data)}"


def _write_wheel(wheel_directory: str, *, editable: bool) -> str:
    project = _meta()
    dist = _dist_name(project["name"])
    version = project["version"]
    dist_info = f"{dist}-{version}.dist-info"
    wheel_name = f"{dist}-{version}-py3-none-any.whl"
    wheel_path = Path(wheel_directory) / wheel_name

    records: list[str] = []
    with zipfile.ZipFile(wheel_path, "w", compression=zipfile.ZIP_DEFLATED) as zf:
        def add_bytes(arcname: str, data: bytes) -> None:
            info = zipfile.ZipInfo(arcname, date_time=(2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            info.external_attr = 0o644 << 16
            zf.writestr(info, data)
            records.append(_record_line(arcname, data))

        if editable:
            add_bytes(f"{dist}.pth", (str(SRC_DIR.resolve()) + "\n").encode("utf-8"))
        else:
            pkg_root = SRC_DIR / PKG_NAME
            for path in sorted(pkg_root.rglob("*.py")):
                if "__pycache__" in path.parts:
                    continue
                rel = path.relative_to(SRC_DIR).as_posix()
                add_bytes(rel, path.read_bytes())

        add_bytes(f"{dist_info}/METADATA", _metadata_text(project).encode("utf-8"))
        add_bytes(f"{dist_info}/WHEEL", _wheel_text().encode("utf-8"))
        entry_points = _entry_points_text(project)
        if entry_points:
            add_bytes(f"{dist_info}/entry_points.txt", entry_points.encode("utf-8"))
        license_file = ROOT / "LICENSE"
        if license_file.is_file():
            add_bytes(f"{dist_info}/LICENSE", license_file.read_bytes())

        record_name = f"{dist_info}/RECORD"
        records.append(f"{record_name},,")
        zf.writestr(record_name, ("\n".join(records) + "\n").encode("utf-8"))
    return wheel_name


def get_requires_for_build_wheel(config_settings=None):
    return []


def get_requires_for_build_editable(config_settings=None):
    return []


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    return _write_wheel(wheel_directory, editable=False)


def build_editable(wheel_directory, config_settings=None, metadata_directory=None):
    return _write_wheel(wheel_directory, editable=True)
