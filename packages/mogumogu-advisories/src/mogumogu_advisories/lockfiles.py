"""Bounded, static, symlink-safe lockfile parsers across all 12 OSV ecosystems.

No package managers, interpreters, or shell commands are ever executed.
Non-public, local, Git, or range-only entries are counted as `skipped` so
coverage gaps remain explicit.
"""
from dataclasses import dataclass, field
import json
import os
from pathlib import Path
import re
import tomllib
from urllib.parse import urlparse

from .models import Package, WatchError

MAX_LOCKFILE_BYTES = 16 << 20
MAX_DISCOVERED_FILES = 500
MAX_WALK_DEPTH = 12

CRATES_IO_SOURCE = "registry+https://github.com/rust-lang/crates.io-index"
JULIA_GENERAL_UUID = "23338594-aafe-5451-b93e-139f81909106"

PUBLIC_NPM_HOSTS = {"registry.npmjs.org", "registry.yarnpkg.com", "npm.pkg.github.com"}
PUBLIC_PYPI_HOSTS = {"pypi.org", "files.pythonhosted.org"}
PUBLIC_PACKAGIST_HOSTS = {
    "packagist.org", "repo.packagist.org", "api.github.com", "github.com",
    "codeload.github.com", "gitlab.com", "bitbucket.org",
}
PUBLIC_RUBYGEMS_HOSTS = {"rubygems.org", "index.rubygems.org"}
PUBLIC_PUB_HOSTS = {"pub.dev", "pub.dartlang.org"}
PUBLIC_CRAN_HOSTS = {
    "cloud.r-project.org", "cran.r-project.org", "cran.rstudio.com",
    "packagemanager.posit.co", "packagemanager.rstudio.com",
}

SKIP_DIRS = frozenset({
    ".git", ".hg", ".svn", "node_modules", "target", ".venv", "venv", "vendor",
    ".tox", ".mypy_cache", ".pytest_cache", "__pycache__", "dist", "build",
    ".dart_tool", "_build", ".build", "Pods", "deps", ".gradle", ".idea", ".vs",
})


@dataclass
class ScanResult:
    packages: set[Package] = field(default_factory=set)
    locations: dict[Package, set[str]] = field(default_factory=dict)
    lockfiles: list[str] = field(default_factory=list)
    skipped: int = 0
    warnings: list[str] = field(default_factory=list)

    def add(self, package: Package | None, location: str) -> None:
        if package is None:
            self.skipped += 1
            return
        self.packages.add(package)
        self.locations.setdefault(package, set()).add(location)

    def merge(self, other: "ScanResult") -> None:
        self.packages.update(other.packages)
        for pkg, locs in other.locations.items():
            self.locations.setdefault(pkg, set()).update(locs)
        self.lockfiles.extend(other.lockfiles)
        self.skipped += other.skipped
        self.warnings.extend(other.warnings)


def _url_host(url: str) -> str | None:
    if not isinstance(url, str) or "://" not in url:
        return None
    parsed = urlparse(url)
    return parsed.hostname.lower() if parsed.hostname else None


def _read_bounded_text(path: Path) -> str:
    if path.is_symlink():
        raise WatchError("Symbolische Verknüpfung wird beim Lockfile-Lesen nicht verfolgt.")
    stat = path.stat()
    if stat.st_size > MAX_LOCKFILE_BYTES:
        raise WatchError(f"Lockfile überschreitet Größenlimit ({MAX_LOCKFILE_BYTES // (1 << 20)} MiB).")
    return path.read_text(encoding="utf-8", errors="strict")


def _parse_cargo_lock_counts(text: str) -> tuple[set[Package], int, int]:
    data = tomllib.loads(text)
    packages: set[Package] = set()
    non_registry = 0
    invalid = 0
    for entry in data.get("package", []):
        if entry.get("source") != CRATES_IO_SOURCE:
            non_registry += 1
            continue
        pkg = Package.parse({"ecosystem": "crates.io", "name": entry.get("name", ""), "version": entry.get("version", "")})
        if pkg is None:
            invalid += 1
        else:
            packages.add(pkg)
    return packages, non_registry, invalid


def parse_cargo_lock_detailed(path: Path) -> tuple[set[Package], int, int]:
    """Parses Cargo.lock into `(packages, non_registry_skipped, invalid_skipped)`."""
    return _parse_cargo_lock_counts(_read_bounded_text(Path(path)))


def _parse_cargo_lock(text: str, location: str) -> ScanResult:
    packages, non_registry, invalid = _parse_cargo_lock_counts(text)
    return ScanResult(
        packages=packages,
        locations={pkg: {location} for pkg in packages},
        lockfiles=[location],
        skipped=non_registry + invalid,
    )


def _npm_name_from_path(pkg_path: str) -> str:
    marker = "node_modules/"
    idx = pkg_path.rfind(marker)
    return pkg_path[idx + len(marker):] if idx >= 0 else pkg_path


def _parse_package_lock_json(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = json.loads(text)
    packages_obj = data.get("packages")
    if isinstance(packages_obj, dict):
        for pkg_path, entry in packages_obj.items():
            if not pkg_path or not isinstance(entry, dict):
                continue
            if entry.get("link") is True:
                out.skipped += 1
                continue
            resolved = entry.get("resolved")
            if isinstance(resolved, str):
                host = _url_host(resolved)
                if host is None or host not in PUBLIC_NPM_HOSTS:
                    out.skipped += 1
                    continue
            name = entry.get("name") or _npm_name_from_path(pkg_path)
            out.add(Package.parse({"ecosystem": "npm", "name": name, "version": entry.get("version", "")}), location)
        return out

    def walk_v1(deps: dict, depth: int = 0) -> None:
        if depth > 32 or not isinstance(deps, dict):
            return
        for name, entry in deps.items():
            if not isinstance(entry, dict):
                continue
            resolved = entry.get("resolved")
            if isinstance(resolved, str):
                host = _url_host(resolved)
                if host is None or host not in PUBLIC_NPM_HOSTS:
                    out.skipped += 1
                    continue
            out.add(Package.parse({"ecosystem": "npm", "name": name, "version": entry.get("version", "")}), location)
            walk_v1(entry.get("dependencies", {}), depth + 1)

    walk_v1(data.get("dependencies", {}))
    return out


def _parse_pnpm_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    in_packages = False
    for raw in text.splitlines()[:500_000]:
        if not raw.strip() or raw.lstrip().startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        stripped = raw.strip()
        if indent == 0:
            in_packages = stripped == "packages:"
            continue
        if not in_packages or indent != 2 or not stripped.endswith(":"):
            continue
        key = stripped[:-1].strip().strip("'\"")
        if key.startswith(("file:", "link:", "git+", "http://", "https://")) or "file:" in key:
            out.skipped += 1
            continue
        # v6+: /@scope/pkg@1.2.3(peer) or /pkg@1.2.3 ; v9: @scope/pkg@1.2.3 or pkg@1.2.3
        key = key.lstrip("/")
        key = key.split("(", 1)[0]
        if "@" not in key:
            out.skipped += 1
            continue
        if key.startswith("@"):
            parts = key[1:].split("@", 1)
            if len(parts) != 2:
                out.skipped += 1
                continue
            name, version = "@" + parts[0], parts[1]
        else:
            name, version = key.rsplit("@", 1)
        out.add(Package.parse({"ecosystem": "npm", "name": name, "version": version}), location)
    return out


def _parse_uv_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = tomllib.loads(text)
    for entry in data.get("package", []):
        source = entry.get("source")
        if not isinstance(source, dict) or "registry" not in source:
            out.skipped += 1
            continue
        host = _url_host(source["registry"])
        if host not in PUBLIC_PYPI_HOSTS:
            out.skipped += 1
            continue
        out.add(
            Package.parse({"ecosystem": "PyPI", "name": entry.get("name", ""), "version": entry.get("version", "")}),
            location,
        )
    return out


def _parse_poetry_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = tomllib.loads(text)
    for entry in data.get("package", []):
        source = entry.get("source")
        if isinstance(source, dict):
            src_type = source.get("type", "")
            if src_type in ("directory", "file", "git", "url"):
                out.skipped += 1
                continue
            url = source.get("url")
            if isinstance(url, str) and _url_host(url) not in PUBLIC_PYPI_HOSTS:
                out.skipped += 1
                continue
        out.add(
            Package.parse({"ecosystem": "PyPI", "name": entry.get("name", ""), "version": entry.get("version", "")}),
            location,
        )
    return out


_REQ_EXACT = re.compile(r"^\s*([A-Za-z0-9][A-Za-z0-9_.-]*)(?:\[[^\]]+\])?\s*==\s*([^\s;#\\]+)")


def _parse_requirements_txt(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    for raw in text.splitlines()[:100_000]:
        line = raw.split("#", 1)[0].strip()
        if not line or line.startswith(("--hash=", "\\")):
            continue
        if line.startswith(("-", "git+", "http://", "https://", "file:", ".", "/")):
            out.skipped += 1
            continue
        m = _REQ_EXACT.match(line)
        if not m:
            out.skipped += 1
            continue
        out.add(Package.parse({"ecosystem": "PyPI", "name": m.group(1), "version": m.group(2)}), location)
    return out


def _parse_go_mod(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    in_require = False
    in_replace = False
    requires: list[tuple[str, str]] = []
    local_replaces: set[str] = set()
    for raw in text.splitlines()[:100_000]:
        line = raw.split("//", 1)[0].strip()
        if not line:
            continue
        if line == ")":
            in_require = False
            in_replace = False
            continue
        if line.startswith("require ("):
            in_require = True
            continue
        if line.startswith("replace ("):
            in_replace = True
            continue
        if line.startswith("require "):
            tokens = line[len("require "):].strip().split()
            if len(tokens) >= 2:
                requires.append((tokens[0].strip('"'), tokens[1]))
            continue
        if line.startswith("replace "):
            body = line[len("replace "):].strip()
            if "=>" in body:
                left, right = body.split("=>", 1)
                if right.strip().startswith((".", "/", "\\")):
                    local_replaces.add(left.strip().split()[0].strip('"'))
            continue
        if in_require:
            tokens = line.split()
            if len(tokens) >= 2:
                requires.append((tokens[0].strip('"'), tokens[1]))
        elif in_replace and "=>" in line:
            left, right = line.split("=>", 1)
            if right.strip().startswith((".", "/", "\\")):
                local_replaces.add(left.strip().split()[0].strip('"'))
    for mod, ver in requires:
        if mod in local_replaces:
            out.skipped += 1
            continue
        out.add(Package.parse({"ecosystem": "Go", "name": mod, "version": ver}), location)
    return out


def _parse_nuget_packages_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = json.loads(text)
    deps = data.get("dependencies")
    if not isinstance(deps, dict):
        return out
    for _tfm, entries in deps.items():
        if not isinstance(entries, dict):
            continue
        for name, meta in entries.items():
            if not isinstance(meta, dict):
                continue
            if meta.get("type") == "Project":
                out.skipped += 1
                continue
            ver = meta.get("resolved", "")
            out.add(Package.parse({"ecosystem": "NuGet", "name": name, "version": ver}), location)
    return out


def _parse_gradle_lockfile(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    for raw in text.splitlines()[:100_000]:
        line = raw.strip()
        if not line or line.startswith("#") or line.startswith("empty="):
            continue
        coord = line.split("=", 1)[0]
        parts = coord.split(":")
        if len(parts) != 3:
            out.skipped += 1
            continue
        name = f"{parts[0]}:{parts[1]}"
        out.add(Package.parse({"ecosystem": "Maven", "name": name, "version": parts[2]}), location)
    return out


def _parse_composer_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = json.loads(text)
    for section in ("packages", "packages-dev"):
        for entry in data.get(section, []):
            if not isinstance(entry, dict):
                continue
            name = entry.get("name", "")
            if "/" not in name:
                continue
            dist = entry.get("dist") or {}
            source = entry.get("source") or {}
            if dist.get("type") == "path" or source.get("type") == "path":
                out.skipped += 1
                continue
            notif_host = _url_host(entry.get("notification-url", ""))
            if notif_host != "packagist.org":
                dist_url = dist.get("url")
                if isinstance(dist_url, str):
                    if _url_host(dist_url) not in PUBLIC_PACKAGIST_HOSTS:
                        out.skipped += 1
                        continue
                elif source.get("type"):
                    out.skipped += 1
                    continue
            out.add(
                Package.parse({"ecosystem": "Packagist", "name": name, "version": entry.get("version", "")}),
                location,
            )
    return out


def _parse_gemfile_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    section = ""
    remotes: list[str] = []
    in_specs = False
    for raw in text.splitlines()[:200_000]:
        line = raw.rstrip()
        content = line.lstrip(" ")
        if not content:
            continue
        indent = len(line) - len(content)
        if indent == 0:
            section = line
            remotes = []
            in_specs = False
            continue
        if section in ("GEM", "GIT", "PATH", "PLUGIN SOURCE"):
            if indent == 2:
                if content == "specs:":
                    in_specs = True
                elif content.startswith("remote: "):
                    remotes.append(content[len("remote: "):].strip())
            elif indent == 4 and in_specs and " (" in content and content.endswith(")"):
                name, rest = content.split(" (", 1)
                inside = rest[:-1]
                version = inside.split("-", 1)[0]
                if section != "GEM":
                    out.skipped += 1
                    continue
                if remotes and not all(_url_host(r) in PUBLIC_RUBYGEMS_HOSTS for r in remotes):
                    out.skipped += 1
                    continue
                out.add(Package.parse({"ecosystem": "RubyGems", "name": name, "version": version}), location)
    return out


def _parse_pubspec_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    in_packages = False
    current_name: str | None = None
    current_source: str | None = None
    current_url: str | None = None
    current_version: str | None = None

    def flush() -> None:
        nonlocal current_name, current_source, current_url, current_version
        if current_name is None:
            return
        if current_source != "hosted" or (current_url and _url_host(current_url) not in PUBLIC_PUB_HOSTS):
            out.skipped += 1
        else:
            out.add(
                Package.parse({"ecosystem": "Pub", "name": current_name, "version": current_version or ""}),
                location,
            )
        current_name = current_source = current_url = current_version = None

    for raw in text.splitlines()[:200_000]:
        stripped = raw.strip()
        if not stripped or stripped.startswith("#"):
            continue
        indent = len(raw) - len(raw.lstrip(" "))
        if indent == 0:
            flush()
            in_packages = stripped == "packages:"
            continue
        if not in_packages:
            continue
        if indent == 2 and stripped.endswith(":"):
            flush()
            current_name = stripped[:-1].strip().strip("'\"")
        elif indent == 4 and ":" in stripped:
            k, v = stripped.split(":", 1)
            k, v = k.strip(), v.strip().strip("'\"")
            if k == "source":
                current_source = v
            elif k == "version":
                current_version = v
        elif indent == 6 and stripped.startswith("url:"):
            current_url = stripped.split(":", 1)[1].strip().strip("'\"")
    flush()
    return out


_MIX_HEX_ENTRY = re.compile(
    r'^\s*"([^"]+)"\s*:\s*\{:hex,\s*:([A-Za-z0-9_]+),\s*"([^"]+)"(.*)\},?\s*$'
)
_MIX_NON_HEX = re.compile(r'^\s*"([^"]+)"\s*:\s*\{:(git|path)\b')


def _parse_mix_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    for raw in text.splitlines()[:100_000]:
        m = _MIX_HEX_ENTRY.match(raw)
        if m:
            pkg_name, version, tail = m.group(2), m.group(3), m.group(4)
            # Check repo string (e.g. "hexpm" vs "hexpm:org" or custom mirror)
            repos = re.findall(r'"([^"]+)"', tail)
            # In mix.lock format: inner_checksum, deps_strings..., repo, outer_checksum
            # Any quoted "hexpm:<org>" indicates a private organization.
            if any(r.startswith("hexpm:") for r in repos):
                out.skipped += 1
                continue
            out.add(Package.parse({"ecosystem": "Hex", "name": pkg_name, "version": version}), location)
        elif _MIX_NON_HEX.match(raw):
            out.skipped += 1
    return out


def _parse_renv_lock(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = json.loads(text)
    repos = {
        r["Name"]: r["URL"]
        for r in data.get("R", {}).get("Repositories", [])
        if isinstance(r, dict) and isinstance(r.get("Name"), str) and isinstance(r.get("URL"), str)
    }
    packages = data.get("Packages")
    if not isinstance(packages, dict):
        return out
    for key, rec in packages.items():
        if not isinstance(rec, dict):
            continue
        if rec.get("Source") != "Repository":
            out.skipped += 1
            continue
        repo_name = rec.get("Repository", "")
        repo_url = repo_name if "://" in repo_name else repos.get(repo_name, "")
        if _url_host(repo_url) not in PUBLIC_CRAN_HOSTS:
            out.skipped += 1
            continue
        name = rec.get("Package") or key
        out.add(Package.parse({"ecosystem": "CRAN", "name": name, "version": rec.get("Version", "")}), location)
    return out


def _parse_julia_manifest(text: str, location: str) -> ScanResult:
    out = ScanResult(lockfiles=[location])
    data = tomllib.loads(text)
    if "manifest_format" not in data and "julia_version" not in data and "deps" not in data:
        return out
    registries = data.get("registries") if isinstance(data.get("registries"), dict) else {}
    deps = data.get("deps") if "manifest_format" in data else data
    if not isinstance(deps, dict):
        return out
    for name, entries in deps.items():
        if not isinstance(entries, list):
            continue
        for entry in entries:
            if not isinstance(entry, dict) or "uuid" not in entry:
                continue
            if "path" in entry or "repo-url" in entry:
                out.skipped += 1
                continue
            if "git-tree-sha1" not in entry:
                # Standard library shipped with Julia.
                continue
            reg_ref = entry.get("registries")
            if reg_ref is not None:
                reg_name = reg_ref if isinstance(reg_ref, str) else (reg_ref[0] if reg_ref else None)
                reg_meta = registries.get(reg_name) if isinstance(reg_name, str) else None
                if isinstance(reg_meta, dict) and reg_meta.get("uuid") != JULIA_GENERAL_UUID:
                    out.skipped += 1
                    continue
            out.add(Package.parse({"ecosystem": "Julia", "name": name, "version": entry.get("version", "")}), location)
    return out


def parse_package_list(path: Path, *, location: str | None = None) -> ScanResult:
    """Parses an explicit JSON array of `{ecosystem, name, version}` objects."""
    loc = location or path.name
    text = _read_bounded_text(path)
    rows = json.loads(text)
    if not isinstance(rows, list):
        raise WatchError("Paketliste muss eine JSON-Liste sein.")
    out = ScanResult(lockfiles=[loc])
    for row in rows:
        if not isinstance(row, dict):
            out.skipped += 1
            continue
        out.add(Package.parse(row), loc)
    return out


LOCKFILE_PARSERS = {
    "cargo.lock": _parse_cargo_lock,
    "package-lock.json": _parse_package_lock_json,
    "npm-shrinkwrap.json": _parse_package_lock_json,
    "pnpm-lock.yaml": _parse_pnpm_lock,
    "uv.lock": _parse_uv_lock,
    "poetry.lock": _parse_poetry_lock,
    "requirements.txt": _parse_requirements_txt,
    "go.mod": _parse_go_mod,
    "packages.lock.json": _parse_nuget_packages_lock,
    "gradle.lockfile": _parse_gradle_lockfile,
    "composer.lock": _parse_composer_lock,
    "gemfile.lock": _parse_gemfile_lock,
    "gems.locked": _parse_gemfile_lock,
    "pubspec.lock": _parse_pubspec_lock,
    "mix.lock": _parse_mix_lock,
    "renv.lock": _parse_renv_lock,
    "manifest.toml": _parse_julia_manifest,
    "juliamanifest.toml": _parse_julia_manifest,
}
SUPPORTED_LOCKFILES = frozenset(LOCKFILE_PARSERS)


def _is_versioned_julia_manifest(lower_name: str) -> bool:
    return lower_name.startswith(("manifest-v", "juliamanifest-v")) and lower_name.endswith(".toml")


def parse_lockfile(path: Path, *, base_dir: Path | None = None) -> ScanResult:
    """Parses one supported lockfile statically without executing anything."""
    path = Path(path)
    if base_dir is not None:
        try:
            location = path.resolve().relative_to(base_dir.resolve()).as_posix()
        except ValueError:
            location = path.name
    else:
        location = path.name

    lower = path.name.lower()
    parser = LOCKFILE_PARSERS.get(lower)
    if parser is None and _is_versioned_julia_manifest(lower):
        parser = _parse_julia_manifest
    if parser is None:
        raise WatchError(f"Lockfile-Format nicht unterstützt: {path.name}")
    return parser(_read_bounded_text(path), location)


def discover_lockfiles(root: Path) -> list[Path]:
    """Finds supported lockfiles under `root` without following symlinks."""
    root = Path(root).resolve()
    if root.is_file():
        return [root]
    found: list[Path] = []
    for current, subdirs, filenames in os.walk(root, followlinks=False):
        current_path = Path(current)
        depth = len(current_path.relative_to(root).parts)
        if depth >= MAX_WALK_DEPTH:
            subdirs[:] = []
        else:
            subdirs[:] = sorted(
                d for d in subdirs
                if d not in SKIP_DIRS and not (current_path / d).is_symlink()
            )
        for name in sorted(filenames):
            lower = name.lower()
            if lower in SUPPORTED_LOCKFILES or _is_versioned_julia_manifest(lower):
                candidate = current_path / name
                if not candidate.is_symlink():
                    found.append(candidate)
                    if len(found) >= MAX_DISCOVERED_FILES:
                        return found
    return found


def scan_lockfiles(target: Path, *, explicit_files: list[Path] | None = None) -> ScanResult:
    """Scans a project directory, a single lockfile, or explicit lockfiles."""
    target = Path(target).resolve()
    base_dir = target if target.is_dir() else target.parent
    files = [Path(p).resolve() for p in explicit_files] if explicit_files else discover_lockfiles(target)
    combined = ScanResult()
    for file_path in files:
        try:
            combined.merge(parse_lockfile(file_path, base_dir=base_dir))
        except (OSError, ValueError, json.JSONDecodeError, tomllib.TOMLDecodeError, WatchError) as error:
            rel = file_path.name
            try:
                rel = file_path.relative_to(base_dir).as_posix()
            except ValueError:
                pass
            combined.warnings.append(f"{rel}: {error}")
            combined.skipped += 1
    return combined
