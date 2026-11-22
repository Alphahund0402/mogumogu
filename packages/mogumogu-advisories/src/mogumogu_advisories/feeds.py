"""Local OSV vulnerability archive verification, freshness tracking, and download."""
from datetime import datetime, timezone
from email.utils import parsedate_to_datetime
from http.client import HTTPException
import os
from pathlib import Path
import tempfile
import time
from urllib.error import HTTPError
from urllib.parse import quote
from urllib.request import urlopen
import zipfile

from .models import ECOSYSTEMS, WatchError

DAY = 86400
MAX_AGE = 7 * DAY
MAX_ARCHIVE = 512 << 20
MAX_ARCHIVE_EXPANDED = 2 << 30
DOWNLOAD_SECONDS = 600


def default_cache_dir() -> Path:
    """Returns the default local cache directory for OSV archives and scanner binaries."""
    env = os.environ.get("MOGUMOGU_OSV_CACHE")
    if env:
        return Path(env)
    if os.name == "nt":
        base = Path(os.environ.get("LOCALAPPDATA") or (Path.home() / "AppData" / "Local"))
        return base / "mogumogu-advisories"
    xdg = os.environ.get("XDG_CACHE_HOME")
    base = Path(xdg) if xdg else (Path.home() / ".cache")
    return base / "mogumogu-advisories"


def db_path(cache: Path, ecosystem: str) -> Path:
    return cache / "osv-scalibr" / ecosystem / "all.zip"


def check_archive(path: Path, verify_contents: bool) -> None:
    with zipfile.ZipFile(path) as archive:
        entries = archive.infolist()
        if (
            len(entries) > 500_000
            or sum(entry.file_size for entry in entries) > MAX_ARCHIVE_EXPANDED
            or not any(entry.filename.endswith(".json") and entry.file_size for entry in entries)
        ):
            raise WatchError("OSV-Datenbank überschreitet Archivlimit oder enthält keine Datensätze.")
        if verify_contents and archive.testzip() is not None:
            raise WatchError("OSV-Datenbank ist unvollständig.")


def feed_status(path: Path, now: float, max_age_seconds: float = MAX_AGE) -> str:
    try:
        stat = path.stat()
        if not path.is_file() or stat.st_size > MAX_ARCHIVE:
            return "defekt"
        check_archive(path, verify_contents=False)
    except FileNotFoundError:
        return "fehlt"
    except OSError:
        return "nicht_lesbar"
    except (WatchError, zipfile.BadZipFile):
        return "defekt"
    return "veraltet" if now - stat.st_mtime > max_age_seconds else "aktuell"


def feed_info(cache: Path, ecosystem: str, now: float | None = None, max_age_days: int = 7) -> dict:
    now = time.time() if now is None else now
    archive = db_path(cache, ecosystem)
    max_age_seconds = max(1, max_age_days) * DAY
    status = feed_status(archive, now, max_age_seconds=max_age_seconds)
    info = {
        "ecosystem": ecosystem,
        "status": status,
        "modified_at": None,
        "age_days": None,
        "max_age_days": max(1, max_age_days),
    }
    if status in ("aktuell", "veraltet"):
        mtime = archive.stat().st_mtime
        age_days = max(0.0, (now - mtime) / DAY)
        info["modified_at"] = datetime.fromtimestamp(mtime, timezone.utc).isoformat(timespec="seconds")
        info["age_days"] = round(age_days, 2)
    return info


def download_feed(path: Path, ecosystem: str, *, opener=None) -> None:
    url = f"https://osv-vulnerabilities.storage.googleapis.com/{quote(ecosystem, safe='.')}/all.zip"
    path.parent.mkdir(parents=True, exist_ok=True)
    fetch = urlopen if opener is None else opener
    with tempfile.TemporaryDirectory(dir=path.parent) as staging:
        pending = Path(staging) / "all.zip"
        deadline = time.monotonic() + DOWNLOAD_SECONDS
        with fetch(url, timeout=15) as response, pending.open("wb") as out:
            if response.geturl() != url:
                raise WatchError("Unerwartete Weiterleitung der OSV-Quelle.")
            modified = response.headers.get("Last-Modified")
            length = response.headers.get("Content-Length")
            if length and int(length) > MAX_ARCHIVE:
                raise WatchError("OSV-Datenbank überschreitet das Downloadlimit.")
            size = 0
            while chunk := response.read1(1 << 20):
                size += len(chunk)
                if size > MAX_ARCHIVE:
                    raise WatchError("OSV-Datenbank überschreitet das Downloadlimit.")
                if time.monotonic() > deadline:
                    raise WatchError("Zeitlimit für OSV-Download erreicht.")
                out.write(chunk)
        check_archive(pending, verify_contents=True)
        if modified:
            published = parsedate_to_datetime(modified).timestamp()
            os.utime(pending, (published, published))
        pending.replace(path)


def refresh_feeds(cache: Path, *, only_due: bool = False, ecosystems=ECOSYSTEMS, opener=None) -> dict[str, str]:
    failed: dict[str, str] = {}
    for ecosystem in ecosystems:
        path = db_path(cache, ecosystem)
        try:
            if not only_due or not path.is_file() or time.time() - path.stat().st_mtime >= DAY:
                download_feed(path, ecosystem, opener=opener)
        except WatchError as error:
            failed[ecosystem] = str(error)
        except HTTPError as error:
            failed[ecosystem] = f"OSV-Download meldet HTTP {error.code}."
        except (OSError, HTTPException, ValueError, RuntimeError, zipfile.BadZipFile):
            failed[ecosystem] = "OSV-Datenbank konnte nicht geladen oder geprüft werden."
    return failed
