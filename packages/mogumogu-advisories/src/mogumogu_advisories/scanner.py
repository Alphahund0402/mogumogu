"""SHA-256-verified OSV-Scanner download and offline package matching."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import subprocess
import tempfile
from urllib.parse import urlparse
from urllib.request import urlopen

from .models import ADVISORY_ID, CVE_ID, Advisory, Package, WatchError

OSV_SCANNER_VERSION = "v2.7.1"
MAX_BINARY_BYTES = 128 << 20
MAX_RESULTS = 64 << 20

# Source: https://github.com/google/osv-scanner/releases/download/v2.7.1/osv-scanner_SHA256SUMS
OSV_SCANNER_CHECKSUMS: dict[tuple[str, str], tuple[str, str]] = {
    ("linux", "amd64"): (
        "osv-scanner_linux_amd64",
        "fb7dab19a8bd9bbe5c3189f09d4b7ee1820c9553a23bac202648a56efd2f6ee8",
    ),
    ("linux", "arm64"): (
        "osv-scanner_linux_arm64",
        "983d22fbad9724514e3fcc6acc83b5fa64c5ffa6b0d9f510028f22c1b223ff81",
    ),
    ("windows", "amd64"): (
        "osv-scanner_windows_amd64.exe",
        "7121dcf420aefbbf89a7f0050c4d3094e079ff93afd334dfc96f16b02ec85800",
    ),
    ("windows", "arm64"): (
        "osv-scanner_windows_arm64.exe",
        "5f3998b55c9268f8468502bbcb60645038fd5b145ee5e685e6ae2a32e923afc7",
    ),
    ("darwin", "amd64"): (
        "osv-scanner_darwin_amd64",
        "b8420d5485ba792626315f1bf9a5a2046e971d38eea75bf9090c38852dbc3b1b",
    ),
    ("darwin", "arm64"): (
        "osv-scanner_darwin_arm64",
        "b22c02877a811c8f78956e8643efa554932b7378f590d13f8e97d4e9448a29f3",
    ),
}


def detect_platform() -> tuple[str, str]:
    sys_name = platform.system().lower()
    os_key = "windows" if sys_name.startswith("win") else ("darwin" if sys_name == "darwin" else "linux")
    machine = platform.machine().lower()
    arch_key = "arm64" if machine in ("arm64", "aarch64") else "amd64"
    return os_key, arch_key


def default_scanner_path(cache: Path) -> Path:
    """Returns the default verified OSV-Scanner path from env or inside the cache."""
    env = os.environ.get("MOGUMOGU_OSV_SCANNER")
    if env:
        return Path(env)
    name = "osv-scanner.exe" if os.name == "nt" else "osv-scanner"
    return Path(cache) / "bin" / name


def download_scanner(
    output: Path,
    *,
    os_name: str | None = None,
    arch: str | None = None,
    opener=None,
) -> tuple[str, str, int]:
    """Downloads and SHA-256-verifies the pinned official OSV-Scanner release."""
    default_os, default_arch = detect_platform()
    key = ((os_name or default_os).lower(), (arch or default_arch).lower())
    if key not in OSV_SCANNER_CHECKSUMS:
        raise WatchError(f"Nicht unterstützte Plattform {key!r} für OSV-Scanner {OSV_SCANNER_VERSION}.")
    asset, expected_sha256 = OSV_SCANNER_CHECKSUMS[key]
    url = f"https://github.com/google/osv-scanner/releases/download/{OSV_SCANNER_VERSION}/{asset}"
    output = Path(output).resolve()
    output.parent.mkdir(parents=True, exist_ok=True)
    fetch = urlopen if opener is None else opener
    with tempfile.TemporaryDirectory(dir=output.parent) as staging:
        pending = Path(staging) / asset
        digest = hashlib.sha256()
        total = 0
        with fetch(url, timeout=60) as response, pending.open("wb") as dest:
            final_url = urlparse(response.geturl())
            if final_url.scheme != "https":
                raise WatchError(f"Nicht-HTTPS-Weiterleitung abgelehnt ({final_url.scheme}).")
            while chunk := response.read(1 << 20):
                total += len(chunk)
                if total > MAX_BINARY_BYTES:
                    raise WatchError("OSV-Scanner-Binary überschreitet das Größenlimit.")
                digest.update(chunk)
                dest.write(chunk)
        actual_sha256 = digest.hexdigest()
        if actual_sha256 != expected_sha256:
            raise WatchError(
                f"SHA-256-Prüfsumme für {asset} {OSV_SCANNER_VERSION} stimmt nicht überein "
                f"(erwartet {expected_sha256}, erhalten {actual_sha256})."
            )
        if key[0] != "windows":
            os.chmod(pending, 0o755)
        pending.replace(output)
    return asset, expected_sha256, total


def executable(path: str | Path) -> Path:
    resolved = Path(path).resolve()
    if not resolved.is_file() or (os.name == "nt" and resolved.suffix.lower() != ".exe"):
        raise argparse.ArgumentTypeError("Pfad zu einer vertrauenswürdigen ausführbaren Datei erforderlich.")
    return resolved


def read_matches(result: dict, input_packages: set[Package]) -> dict[Package, set[Advisory]]:
    checked: dict[Package, set[Advisory]] = {}
    for source in result.get("results", []):
        for row in source.get("packages", []):
            package = Package.parse(row.get("package", {}))
            if package not in input_packages:
                continue
            present = {
                vuln["id"]: {alias for alias in [vuln["id"], *vuln.get("aliases", [])] if CVE_ID.fullmatch(alias)}
                for vuln in row.get("vulnerabilities", [])
                if not vuln.get("withdrawn") and ADVISORY_ID.fullmatch(vuln["id"])
            }
            groups: set[tuple[str, ...]] = set()
            for group in row.get("groups", []):
                ids = present.keys() & set(group.get("ids", []))
                if ids:
                    groups.add(tuple(sorted(ids)))
            grouped = {advisory for group in groups for advisory in group}
            groups.update((advisory,) for advisory in present.keys() - grouped)
            checked.setdefault(package, set()).update(
                Advisory(ids, tuple(sorted({cve for advisory in ids for cve in present[advisory]})))
                for ids in groups
            )
    return checked


def match_packages(
    scanner: Path,
    cache: Path,
    packages: set[Package],
    *,
    runner=None,
) -> dict[Package, set[Advisory]]:
    """Matches exact packages offline via the verified OSV-Scanner binary."""
    if not packages:
        return {}
    run = subprocess.run if runner is None else runner
    with tempfile.TemporaryDirectory(prefix="mogumogu-advisories-") as directory:
        root = Path(directory)
        rows = [{"package": package.fields()} for package in sorted(packages)]
        (root / "inventory.json").write_text(json.dumps({"results": [{"packages": rows}]}), encoding="utf-8")
        (root / "config.toml").write_text("", encoding="utf-8")
        env = os.environ.copy()
        env["OSV_SCANNER_LOCAL_DB_CACHE_DIRECTORY"] = str(cache)
        command = [
            str(scanner), "scan", "source", "--offline", "--no-call-analysis=all", "--all-packages",
            "--format=json", "--config=config.toml", "--lockfile=osv-scanner:inventory.json",
        ]
        with (root / "results.json").open("w+b") as output:
            result = run(
                command, cwd=root, env=env, stdout=output, stderr=subprocess.DEVNULL, timeout=120, check=False,
            )
            if result.returncode not in (0, 1):
                raise WatchError(f"OSV-Scanner hat den Abgleich abgebrochen (Status {result.returncode}).")
            output.seek(0)
            text = output.read(MAX_RESULTS + 1)
        if len(text) > MAX_RESULTS:
            raise WatchError("Ausgabe des OSV-Scanners überschreitet das Limit.")
        return read_matches(json.loads(text), packages)
