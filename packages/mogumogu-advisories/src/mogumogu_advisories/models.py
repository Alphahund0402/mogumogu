"""Core data models and strict package identity validation for OSV matching."""
from dataclasses import dataclass
from datetime import date
import re

ECOSYSTEMS = ("npm", "PyPI", "crates.io", "Go")
PROJECT_ECOSYSTEMS = (
    *ECOSYSTEMS,
    "NuGet",
    "Maven",
    "Packagist",
    "RubyGems",
    "Pub",
    "Hex",
    "CRAN",
    "Julia",
)

SEMVER_NUMBER = r"(?:0|[1-9][0-9]*)"
SEMVER_PRE = rf"(?:{SEMVER_NUMBER}|(?=[0-9a-zA-Z-]*[a-zA-Z-])[0-9a-zA-Z-]+)"
SEMVER = (
    rf"{SEMVER_NUMBER}(?:\.{SEMVER_NUMBER}){{2}}"
    rf"(?:-{SEMVER_PRE}(?:\.{SEMVER_PRE})*)?"
    r"(?:\+[0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*)?"
)
PEP440 = (
    r"(?:[0-9]+!)?[0-9]+(?:\.[0-9]+)*(?:(?:a|b|rc)[0-9]+)?"
    r"(?:\.post[0-9]+)?(?:\.dev[0-9]+)?(?:\+[a-z0-9]+(?:\.[a-z0-9]+)*)?"
)
ADVISORY_ID = re.compile(r"[A-Za-z0-9][A-Za-z0-9_.-]{0,99}")
CVE_ID = re.compile(r"CVE-[0-9]{4}-[0-9]{4,}")
MODULE_PATH = r"[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)+(?:/[A-Za-z0-9][A-Za-z0-9_.-]*)+"
MAVEN_COORDINATE = r"[A-Za-z0-9_][A-Za-z0-9_.-]*:[A-Za-z0-9_][A-Za-z0-9_.-]*"
NUGET_VERSION = (
    r"[0-9]+(?:\.[0-9]+){0,3}"
    r"(?:-[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?"
    r"(?:\+[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?"
)
PACKAGIST_NAME = r"[a-z0-9](?:[_.-]?[a-z0-9]+)*/[a-z0-9](?:(?:_|-{1,2}|\.)?[a-z0-9]+)*"
PACKAGIST_VERSION = (
    r"[0-9]+(?:\.[0-9]+){1,3}"
    r"(?:-(?!dev(?:$|[.-]))[A-Za-z0-9]+(?:\.[A-Za-z0-9]+)*)?"
    r"(?:\+[A-Za-z0-9-]+(?:\.[A-Za-z0-9-]+)*)?"
)
RUBYGEMS_VERSION = r"[0-9]+(?:\.[0-9A-Za-z]+)*"
CRAN_VERSION = r"[0-9]+(?:[.-][0-9]+)+"


class WatchError(Exception):
    """A bounded error message safe for stdout/stderr without paths or secrets."""


def normalize_package_name(ecosystem: str, name: str) -> str:
    """Normalizes a package name according to its OSV ecosystem rules."""
    if ecosystem == "PyPI":
        return re.sub(r"[-_.]+", "-", name.lower())
    if ecosystem == "crates.io":
        return name.lower().replace("_", "-")
    if ecosystem in ("NuGet", "Packagist", "Pub", "Hex"):
        return name.lower()
    return name


def normalize_version(ecosystem: str, version: str) -> str:
    """Normalizes known exact-version prefixes without turning ranges into versions."""
    if ecosystem == "Go":
        return version.removeprefix("go").removeprefix("v")
    if ecosystem == "Packagist" and version.startswith(("v", "V")):
        return version[1:]
    return version


@dataclass(frozen=True, order=True)
class Evidence:
    scope_id: int
    state: str
    public_source: bool


@dataclass(frozen=True, order=True)
class Advisory:
    ids: tuple[str, ...]
    cves: tuple[str, ...] = ()


@dataclass(frozen=True, order=True)
class TriageEntry:
    advisory_id: str
    ecosystem: str
    package: str
    version: str
    scope: str
    kind: str
    expires: date
    reason: str

    @property
    def crate(self) -> str:
        """Backwards-compatible alias for crates.io triage entries."""
        return self.package


@dataclass(frozen=True, order=True)
class Package:
    ecosystem: str
    name: str
    version: str

    @classmethod
    def parse(cls, row: dict) -> "Package | None":
        ecosystem, name, version = (row.get(field) for field in ("ecosystem", "name", "version"))
        if ecosystem not in PROJECT_ECOSYSTEMS or not all(
            isinstance(value, str) and 0 < len(value) <= 200 for value in (name, version)
        ):
            return None
        name = normalize_package_name(ecosystem, name)
        version = normalize_version(ecosystem, version)
        version_pattern = SEMVER
        if ecosystem == "PyPI":
            valid = re.fullmatch(r"[a-z0-9](?:[a-z0-9-]*[a-z0-9])?", name)
            version_pattern = PEP440
        elif ecosystem == "npm":
            valid = re.fullmatch(r"(?:@[a-z0-9][a-z0-9_.-]*/)?[a-z0-9][a-z0-9_.-]*", name)
        elif ecosystem == "crates.io":
            valid = re.fullmatch(r"[a-z][a-z0-9-]*", name)
        elif ecosystem == "Go":
            valid = name in ("stdlib", "toolchain") or re.fullmatch(MODULE_PATH, name)
        elif ecosystem == "NuGet":
            valid = re.fullmatch(r"[a-z0-9][a-z0-9_.-]*", name)
            version_pattern = NUGET_VERSION
        elif ecosystem == "Maven":
            valid = re.fullmatch(MAVEN_COORDINATE, name) and not version.upper().endswith("-SNAPSHOT")
            version_pattern = r"[0-9][A-Za-z0-9]*(?:[._+-][A-Za-z0-9]+)*"
        elif ecosystem == "Packagist":
            valid = re.fullmatch(PACKAGIST_NAME, name)
            version_pattern = PACKAGIST_VERSION
        elif ecosystem == "RubyGems":
            valid = re.fullmatch(r"[A-Za-z0-9][A-Za-z0-9_.-]*", name)
            version_pattern = RUBYGEMS_VERSION
        elif ecosystem == "Pub":
            valid = re.fullmatch(r"[a-z_][a-z0-9_]*", name)
        elif ecosystem == "Hex":
            valid = re.fullmatch(r"[a-z][a-z0-9_]*", name)
        elif ecosystem == "CRAN":
            valid = re.fullmatch(r"[A-Za-z][A-Za-z0-9.]*[A-Za-z0-9]", name)
            version_pattern = CRAN_VERSION
        else:  # Julia
            valid = re.fullmatch(r"[A-Za-z][A-Za-z0-9_]*", name)
        if not valid or not re.fullmatch(version_pattern, version, re.ASCII):
            return None
        return cls(ecosystem, name, version)

    def fields(self) -> dict[str, str]:
        return {"ecosystem": self.ecosystem, "name": self.name, "version": self.version}
