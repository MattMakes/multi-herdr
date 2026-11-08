"""The record hark keeps for one model request, and how its cache folder is named."""

from __future__ import annotations

import hashlib
import os
import re
from dataclasses import dataclass, field

_SHA_FORM = re.compile(r"^[0-9a-f]{40}$")
_BAD_CHARS = re.compile(r"[^0-9A-Za-z._-]")


def looks_like_sha(text: str | None) -> bool:
    if not text:
        return False
    return _SHA_FORM.match(text) is not None


def absolute_path(where: str) -> str:
    return os.path.abspath(os.path.expanduser(where))


def _sanitise(text: str) -> str:
    return _BAD_CHARS.sub("_", text)


def _dir_fingerprint(where: str, weight_file: str) -> str:
    """Eight hex digits that change when the folder or its weight file changes."""
    root = os.path.normcase(absolute_path(where))
    seed = root
    try:
        info = os.stat(os.path.join(root, weight_file))
    except OSError:
        pass
    else:
        seed = "|".join((root, str(info.st_size), str(info.st_mtime_ns)))
    return hashlib.sha1(seed.encode("utf-8")).hexdigest()[:8]


@dataclass(frozen=True)
class Spec:
    """One model request together with the cache's answer to it."""

    model_id: str
    revision: str
    state: str
    snapshot: str | None
    weights: str
    missing: tuple[str, ...] = field(default_factory=tuple)
    pinned: bool = False
    folder: bool = False

    @property
    def local(self) -> bool:
        return self.snapshot is not None

    @property
    def sha(self) -> str | None:
        if self.folder or self.snapshot is None:
            return None
        return os.path.basename(os.path.normpath(self.snapshot))

    @property
    def label(self) -> str:
        if self.folder:
            return f"the local model folder {self.model_id}"
        shown = self.revision[:7] if looks_like_sha(self.revision) else self.revision
        return f"{self.model_id}@{shown}"

    @property
    def cache_tag(self) -> str:
        if self.pinned:
            return ""
        if self.folder:
            return "--local" + _dir_fingerprint(self.model_id, self.weights)
        return "--" + (self.sha or _sanitise(self.revision))[:8]

    @property
    def cache_name(self) -> str:
        if not self.folder:
            return slug(self.model_id) + self.cache_tag
        leaf = os.path.basename(os.path.normpath(absolute_path(self.model_id)))
        return "local--" + (_sanitise(leaf)[:40] or "model") + self.cache_tag


def slug(model_id: str) -> str:
    """The repo id with each slash turned into a double dash."""
    return "--".join(model_id.split("/"))
