"""Stage 1: fetch a skill source into a read-only quarantine (spec 3.1-3.5, 3.8).

Nothing from the source is ever executed. The only child process is `git`
(clone, plus fetch and checkout for a commit SHA, plus rev-parse), with hooks,
symlinks, submodules, LFS smudge and the file protocol turned off, and a
minimal environment.

Every refusal raises `Refused` with an ACQ finding; the caller records it and
exits 3. A bad source form raises `BadSource`; the caller exits 2.
"""

from __future__ import annotations

import hashlib
import os
import re
import shutil
import stat
import subprocess
import zipfile
from dataclasses import dataclass, field
from pathlib import Path, PurePosixPath

from . import evidence

ACCEPTED_FORMS = """accepted source forms:
  https://github.com/<owner>/<repo>[.git]
  git@github.com:<owner>/<repo>.git
  <owner>/<repo>                      (only when no such local path exists)
  any GitHub form + @<ref>, or --ref <ref>
  https://github.com/<owner>/<repo>/tree/<ref>/<subdir>
  an existing local directory
  a local file ending in .zip or .skill"""

GITHUB = "https://github.com/"
BASE_ENV = "SKILL_INTAKE_GITHUB_BASE"
MB = 1024 * 1024
RATIO_LIMIT = 100
RATIO_MIN_SIZE = 1 * MB

_NAME = r"[A-Za-z0-9._-]+"
_REF = re.compile(r"^[A-Za-z0-9._/-]{1,200}$")
_SHA = re.compile(r"^[0-9a-f]{40}$")
_REPO_FORMS = (
    re.compile(rf"^https?://github\.com/(?P<o>{_NAME})/(?P<r>{_NAME}?)(?:\.git)?/?(?:@(?P<ref>\S+))?$"),
    re.compile(rf"^git@github\.com:(?P<o>{_NAME})/(?P<r>{_NAME}?)(?:\.git)?(?:@(?P<ref>\S+))?$"),
    re.compile(rf"^(?P<o>{_NAME})/(?P<r>{_NAME}?)(?:\.git)?(?:@(?P<ref>\S+))?$"),
)
_TREE_FORM = re.compile(rf"^https?://github\.com/(?P<o>{_NAME})/(?P<r>{_NAME})/tree/(?P<ref>[^/\s]+)(?:/(?P<sub>\S*))?$")


class BadSource(Exception):
    pass


class Refused(Exception):
    """An ACQ-01 or ACQ-04 refusal. `finding` is the record for the sweep."""

    def __init__(self, rule: str, reason: str, file: str | None = None):
        super().__init__(f"{rule}: {reason}")
        self.rule = rule
        self.reason = reason
        self.file = file


@dataclass
class Caps:
    total_bytes: int = 50 * MB
    files: int = 5000
    file_bytes: int = 10 * MB
    depth: int = 32


@dataclass
class Source:
    kind: str  # github | local | archive
    input: str
    url: str | None = None
    ref: str | None = None
    subdir: str | None = None
    path: Path | None = None
    owner: str | None = None
    repo: str | None = None


@dataclass
class Skipped:
    """Files a copy skipped: ACQ-02 symlinks and ACQ-03 special files."""

    items: list[tuple[str, str, str]] = field(default_factory=list)  # (rule, relpath, detail)


def _check_ref(ref: str) -> str:
    if not _REF.match(ref) or ref.startswith(("-", "/")) or ".." in ref:
        raise BadSource(f"bad ref {evidence.escape(ref)!r}: use letters, digits, '.', '_', '-', '/'")
    return ref


def _check_subdir(sub: str) -> str | None:
    sub = sub.strip("/")
    if not sub:
        return None
    p = PurePosixPath(sub)
    if p.is_absolute() or ".." in p.parts or sub.startswith("-") or "\\" in sub:
        raise BadSource(f"bad --path {evidence.escape(sub)!r}: it must be a relative folder inside the source")
    return sub


def parse(text: str, ref: str | None = None, subdir: str | None = None) -> Source:
    """Resolve a source string (spec 3.1). Raise BadSource for any other form."""
    if not text or text != text.strip():
        raise BadSource("empty source or surrounding spaces")
    local = Path(os.path.expanduser(text))
    if local.exists():
        if local.is_dir():
            src = Source("local", text, path=local.resolve())
        elif local.is_file() and local.suffix.lower() in (".zip", ".skill"):
            if subdir:
                raise BadSource("--path applies to a GitHub source or a folder, not to an archive")
            src = Source("archive", text, path=local.resolve())
        else:
            raise BadSource(f"{evidence.escape(text)!r} exists but is not a folder or a .zip/.skill file")
        if ref:
            raise BadSource("--ref applies to a GitHub source only")
        src.subdir = _check_subdir(subdir) if subdir else None
        return src
    m = _TREE_FORM.match(text)
    if m:
        url_ref, url_sub = m.group("ref"), m.group("sub") or ""
        if ref and ref != url_ref:
            raise BadSource("the URL ref and --ref differ")
        if subdir and _check_subdir(subdir) != _check_subdir(url_sub):
            raise BadSource("the URL folder and --path differ")
        return _github(text, m.group("o"), m.group("r"), url_ref, url_sub)
    for form in _REPO_FORMS:
        m = form.match(text)
        if m:
            at_ref = m.group("ref")
            if at_ref and ref and at_ref != ref:
                raise BadSource("the @ref and --ref differ")
            return _github(text, m.group("o"), m.group("r"), at_ref or ref, subdir or "")
    raise BadSource(f"unknown source {evidence.escape(text)!r}")


def _github(text: str, owner: str, repo: str, ref: str | None, sub: str) -> Source:
    if repo.endswith(".git"):
        repo = repo[:-4]
    if not repo or owner.startswith("-") or repo.startswith("-") or owner in (".", "..") or repo in (".", ".."):
        raise BadSource(f"bad owner or repo in {evidence.escape(text)!r}")
    return Source(
        "github", text, url=f"{GITHUB}{owner}/{repo}.git",
        ref=_check_ref(ref) if ref else None, subdir=_check_subdir(sub) if sub else None,
        owner=owner, repo=repo,
    )


# ---- GitHub ---------------------------------------------------------------

# The Claude sandbox routes network through a proxy it sets in these variables.
# git keeps them and nothing else, so a sandboxed pane can clone from GitHub.
_NET_ENV = (
    "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "NO_PROXY",
    "http_proxy", "https_proxy", "all_proxy", "no_proxy",
    "SSL_CERT_FILE", "SSL_CERT_DIR", "GIT_SSL_CAINFO",
)


def _git_env() -> dict[str, str]:
    env = {k: os.environ[k] for k in ("PATH", "HOME", "LANG", *_NET_ENV) if k in os.environ}
    env.update(GIT_LFS_SKIP_SMUDGE="1", GIT_TERMINAL_PROMPT="0", GIT_CONFIG_NOSYSTEM="1")
    return env


def _git_config(file_allowed: bool) -> list[str]:
    return [
        "-c", "core.hooksPath=/dev/null",
        "-c", "core.symlinks=false",
        "-c", f"protocol.file.allow={'always' if file_allowed else 'never'}",
        "-c", "submodule.recurse=false",
    ]


def clone_url(src: Source) -> tuple[str, bool]:
    """The URL to clone and whether it is the file:// test seam."""
    base = os.environ.get(BASE_ENV)
    if not base:
        return src.url, False
    if not base.startswith("file://"):
        raise BadSource(f"{BASE_ENV} must be a file:// URL")
    return base.rstrip("/") + "/" + src.url[len(GITHUB):], True


def _git(args: list[str], cfg: list[str], cwd: Path | None = None) -> str:
    r = subprocess.run(
        ["git", *cfg, *args], cwd=cwd, env=_git_env(),
        stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=600,
    )
    if r.returncode != 0:
        raise BadSource(f"git {args[0]} failed: {evidence.escape(r.stderr.strip())}")
    return r.stdout.strip()


def clone(src: Source, dest: Path) -> str:
    """Shallow clone into `dest` (spec 3.2); return the commit at HEAD."""
    url, file_allowed = clone_url(src)
    cfg = _git_config(file_allowed)
    sha = src.ref if src.ref and _SHA.match(src.ref) else None
    args = ["clone", "--depth", "1", "--no-recurse-submodules", "--single-branch", "--quiet"]
    if src.ref and not sha:
        args += ["--branch", src.ref]
    _git([*args, "--", url, str(dest)], cfg)
    if sha:
        _git(["fetch", "--depth", "1", "--quiet", "origin", sha], cfg, cwd=dest)
        _git(["checkout", "--quiet", "--detach", sha], cfg, cwd=dest)
    return _git(["rev-parse", "HEAD"], cfg, cwd=dest)


# ---- copy with caps -------------------------------------------------------

class _Budget:
    def __init__(self, caps: Caps):
        self.caps = caps
        self.files = 0
        self.bytes = 0

    def path(self, rel: str) -> None:
        if len(PurePosixPath(rel).parts) > self.caps.depth:
            raise Refused("ACQ-04", f"path depth over {self.caps.depth}", rel)

    def file(self, rel: str, size: int) -> None:
        self.path(rel)
        self.files += 1
        self.bytes += size
        if size > self.caps.file_bytes:
            raise Refused("ACQ-04", f"file over {self.caps.file_bytes} bytes", rel)
        if self.files > self.caps.files:
            raise Refused("ACQ-04", f"more than {self.caps.files} files")
        if self.bytes > self.caps.total_bytes:
            raise Refused("ACQ-04", f"total size over {self.caps.total_bytes} bytes")


def copy_tree(root: Path, dest: Path, caps: Caps, skip_git: bool) -> Skipped:
    """Copy regular files only. Symlinks and special files are skipped and listed."""
    budget = _Budget(caps)
    skipped = Skipped()

    def walk(d: Path, rel: PurePosixPath) -> None:
        with os.scandir(d) as it:
            entries = sorted(it, key=lambda e: e.name)
        for e in entries:
            r = rel / e.name
            st = os.lstat(e.path)
            mode = st.st_mode
            if stat.S_ISLNK(mode):
                skipped.items.append(("ACQ-02", r.as_posix(), os.readlink(e.path)))
            elif stat.S_ISDIR(mode):
                if skip_git and e.name == ".git":
                    continue
                budget.path(r.as_posix())
                (dest / r).mkdir()
                walk(Path(e.path), r)
            elif stat.S_ISREG(mode):
                budget.file(r.as_posix(), st.st_size)
                _copy_file(Path(e.path), dest / r, caps.file_bytes)
            else:
                skipped.items.append(("ACQ-03", r.as_posix(), _kind(mode)))

    walk(root, PurePosixPath())
    return skipped


def _kind(mode: int) -> str:
    if stat.S_ISFIFO(mode):
        return "fifo"
    if stat.S_ISSOCK(mode):
        return "socket"
    if stat.S_ISCHR(mode) or stat.S_ISBLK(mode):
        return "device"
    return "special file"


def _copy_file(src: Path, dst: Path, limit: int) -> None:
    # O_NOFOLLOW: the file was a regular file at lstat; never follow a swap.
    fd = os.open(src, os.O_RDONLY | getattr(os, "O_NOFOLLOW", 0))
    with os.fdopen(fd, "rb") as fin, open(dst, "xb") as fout:
        _stream(fin, fout, limit, str(src.name))
    os.chmod(dst, 0o644)


def _stream(fin, fout, limit: int, rel: str) -> int:
    n = 0
    while True:
        chunk = fin.read(65536)
        if not chunk:
            return n
        n += len(chunk)
        if n > limit:
            raise Refused("ACQ-04", f"file over {limit} bytes", rel)
        fout.write(chunk)


# ---- archives -------------------------------------------------------------

def check_archive(zf: zipfile.ZipFile, caps: Caps) -> list[zipfile.ZipInfo]:
    """Refuse the whole archive on any bad entry (ACQ-01) or cap (ACQ-04)."""
    budget = _Budget(caps)
    files = []
    total = 0
    for info in zf.infolist():
        name = info.filename
        norm = name.replace("\\", "/")
        if norm.startswith("/") or re.match(r"^[A-Za-z]:", norm):
            raise Refused("ACQ-01", "absolute path or drive letter in an entry", name)
        if ".." in norm.split("/"):
            raise Refused("ACQ-01", "a '..' segment in an entry", name)
        ftype = (info.external_attr >> 16) & 0o170000
        if ftype == stat.S_IFLNK:
            raise Refused("ACQ-01", "a symlink entry", name)
        if ftype and ftype not in (stat.S_IFREG, stat.S_IFDIR):
            raise Refused("ACQ-01", "a special-file entry", name)
        total += info.file_size
        if total > caps.total_bytes:
            raise Refused("ACQ-01", f"uncompressed size over {caps.total_bytes} bytes")
        if info.file_size > RATIO_MIN_SIZE and info.file_size > RATIO_LIMIT * max(info.compress_size, 1):
            raise Refused("ACQ-01", f"compression ratio over {RATIO_LIMIT}:1", name)
        if info.is_dir():
            budget.path(norm.rstrip("/"))
            continue
        budget.file(norm, info.file_size)
        files.append(info)
    return files


def extract(archive: Path, dest: Path, caps: Caps) -> None:
    try:
        zf = zipfile.ZipFile(archive)
    except (zipfile.BadZipFile, OSError) as e:
        raise Refused("ACQ-01", f"not a readable zip archive ({type(e).__name__})")
    with zf:
        files = check_archive(zf, caps)
        written = 0
        for info in files:
            target = dest / PurePosixPath(info.filename.replace("\\", "/"))
            try:
                target.parent.mkdir(parents=True, exist_ok=True)
                with zf.open(info) as fin, open(target, "xb") as fout:
                    written += _stream(fin, fout, min(caps.file_bytes, info.file_size), info.filename)
            except (OSError, zipfile.BadZipFile) as e:
                # A duplicate or clashing entry name, or a corrupt entry.
                raise Refused("ACQ-01", f"an entry cannot be extracted ({type(e).__name__})", info.filename)
            if written > caps.total_bytes:
                raise Refused("ACQ-01", f"uncompressed size over {caps.total_bytes} bytes")
            os.chmod(target, 0o644)


# ---- a source that is itself a skill -------------------------------------

def source_name(src: Source) -> str:
    """The folder name of the source: the subfolder, the repo, the folder or the archive stem."""
    if src.subdir:
        return PurePosixPath(src.subdir).name
    if src.kind == "github":
        return src.repo or "skill"
    name = src.path.name if src.path else "skill"
    return name.rsplit(".", 1)[0] if src.kind == "archive" else name


def wrap_root_skill(q: Path, name: str) -> str | None:
    """When the source root holds a SKILL.md, move the whole tree into
    q/<name>/, so every skill has a plain rel_path (cr index refuses ".").
    `name` must already be a safe folder name. Return it, or None."""
    if not (q / "SKILL.md").is_file():
        return None
    tmp = q.parent / ".wrap"
    os.replace(q, tmp)
    q.mkdir()
    os.replace(tmp, q / name)
    return name


# ---- hashes and read-only -------------------------------------------------

def sha256_file(path: Path) -> str:
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(65536), b""):
            h.update(chunk)
    return h.hexdigest()


def tree_sha256(root: Path) -> str:
    """sha256 over sorted `relpath\\0sha256\\n` lines (spec 3.3)."""
    lines = []
    for p in root.rglob("*"):
        if p.is_file() and not p.is_symlink():
            lines.append(f"{p.relative_to(root).as_posix()}\0{sha256_file(p)}\n")
    return hashlib.sha256("".join(sorted(lines)).encode("utf-8", "surrogateescape")).hexdigest()


def make_read_only(root: Path) -> None:
    """chmod -R a-w, deepest first."""
    for d, dirs, files in os.walk(root, topdown=False):
        for name in files:
            p = os.path.join(d, name)
            os.chmod(p, stat.S_IMODE(os.lstat(p).st_mode) & ~0o222)
        os.chmod(d, stat.S_IMODE(os.lstat(d).st_mode) & ~0o222)


def make_writable(root: Path) -> None:
    for d, dirs, files in os.walk(root):
        os.chmod(d, stat.S_IMODE(os.lstat(d).st_mode) | 0o200)
        for name in files:
            p = os.path.join(d, name)
            os.chmod(p, stat.S_IMODE(os.lstat(p).st_mode) | 0o200)


def clear(root: Path) -> None:
    """Remove everything under `root` (after a refusal), keeping `root`."""
    if not root.exists():
        return
    make_writable(root)
    for p in root.iterdir():
        if p.is_dir() and not p.is_symlink():
            shutil.rmtree(p)
        else:
            p.unlink()
