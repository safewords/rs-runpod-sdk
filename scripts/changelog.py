#!/usr/bin/env python3
"""Keep CHANGELOG.md in Keep a Changelog 1.1.0 shape, and cut releases from it.

    python3 scripts/changelog.py check            # the format is valid (CI runs this)
    python3 scripts/changelog.py notes 0.2.0      # print the 0.2.0 section (the release notes)
    python3 scripts/changelog.py release 0.2.0    # Unreleased -> [0.2.0] - today, bump Cargo.toml

`release` is the only manual step of a release: commit what it changes and
merge it to `main`, and `.github/workflows/release.yml` tags `v0.2.0` and
publishes the GitHub Release with the section as its notes.

Standard library only.
"""

from __future__ import annotations

import datetime
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
CHANGELOG = ROOT / "CHANGELOG.md"
CARGO = ROOT / "Cargo.toml"
REPO = "https://github.com/safewords/rs-runpod-sdk"

# Keep a Changelog's change types, in its order.
TYPES = ("Added", "Changed", "Deprecated", "Removed", "Fixed", "Security")
SEMVER = r"\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?"
HEADING = re.compile(rf"^## \[(Unreleased|{SEMVER})\](?: - (\d{{4}}-\d{{2}}-\d{{2}}))?$")


def fail(msg: str) -> None:
    sys.exit(f"CHANGELOG.md: {msg}")


def sections(text: str) -> list[tuple[str, str | None, int, int]]:
    """(version, date, start line, end line) for each `## [...]` section."""
    lines = text.split("\n")
    found = []
    for i, line in enumerate(lines):
        if line.startswith("## "):
            m = HEADING.match(line)
            if not m:
                fail(f"line {i + 1}: '{line}' is not '## [Unreleased]' or '## [x.y.z] - YYYY-MM-DD'")
            found.append([m.group(1), m.group(2), i, None])
    end = next((i for i, l in enumerate(lines) if re.match(r"^\[[^\]]+\]: ", l)), len(lines))
    for n, s in enumerate(found):
        s[3] = found[n + 1][2] if n + 1 < len(found) else end
    return [tuple(s) for s in found]


def cargo_version() -> str:
    m = re.search(r'(?m)^version = "([^"]+)"', CARGO.read_text())
    if not m:
        fail("no version in Cargo.toml")
    return m.group(1)


def version_key(v: str) -> tuple:
    core = v.split("-")[0]
    return tuple(int(x) for x in core.split("."))


def check(text: str) -> None:
    if not text.startswith("# Changelog\n"):
        fail("must start with '# Changelog'")
    if "keepachangelog.com/en/1.1.0" not in text:
        fail("must say it follows Keep a Changelog 1.1.0")
    secs = sections(text)
    if not secs or secs[0][0] != "Unreleased":
        fail("the first section must be '## [Unreleased]'")
    lines = text.split("\n")
    versions = [s for s in secs if s[0] != "Unreleased"]
    for v, date, start, end in versions:
        if not date:
            fail(f"[{v}] has no release date")
        datetime.date.fromisoformat(date)
    for a, b in zip(versions, versions[1:]):
        if version_key(a[0]) <= version_key(b[0]):
            fail(f"[{a[0]}] is listed above [{b[0]}]; newest first")
    for v, _, start, end in secs:
        for line in lines[start + 1 : end]:
            if line.startswith("### ") and line[4:] not in TYPES:
                fail(f"[{v}] has '{line}'; change types are {', '.join(TYPES)}")
    if len({s[0] for s in secs}) != len(secs):
        fail("a version appears twice")
    released = cargo_version()
    if versions and version_key(released) < version_key(versions[0][0]):
        fail(f"Cargo.toml says {released}, older than the newest entry [{versions[0][0]}]")


def notes(text: str, version: str) -> str:
    lines = text.split("\n")
    for v, _, start, end in sections(text):
        if v == version:
            body = "\n".join(lines[start + 1 : end]).strip()
            if not body:
                fail(f"[{version}] is empty")
            return body + "\n"
    fail(f"no [{version}] section")
    return ""


def links(versions: list[str]) -> str:
    out = []
    released = [v for v in versions if v != "Unreleased"]
    newest = f"v{released[0]}" if released else "HEAD"
    out.append(f"[Unreleased]: {REPO}/compare/{newest}...HEAD" if released else f"[Unreleased]: {REPO}/commits/main")
    for i, v in enumerate(released):
        if i + 1 < len(released):
            out.append(f"[{v}]: {REPO}/compare/v{released[i + 1]}...v{v}")
        else:
            out.append(f"[{v}]: {REPO}/releases/tag/v{v}")
    return "\n".join(out) + "\n"


def release(text: str, version: str) -> str:
    if not re.fullmatch(SEMVER, version):
        fail(f"'{version}' is not a semantic version")
    secs = sections(text)
    if any(v == version for v, *_ in secs):
        fail(f"[{version}] already exists")
    released = [v for v, *_ in secs if v != "Unreleased"]
    if released and version_key(version) <= version_key(released[0]):
        fail(f"{version} is not newer than [{released[0]}]")
    lines = text.split("\n")
    _, _, start, end = secs[0]
    body = "\n".join(lines[start + 1 : end]).strip()
    if not body:
        fail("[Unreleased] is empty: nothing to release")
    today = datetime.date.today().isoformat()
    head = "\n".join(lines[:start]).rstrip()
    rest_end = next((i for i, l in enumerate(lines) if re.match(r"^\[[^\]]+\]: ", l)), len(lines))
    rest = "\n".join(lines[end:rest_end]).strip()
    new = f"{head}\n\n## [Unreleased]\n\n## [{version}] - {today}\n\n{body}\n"
    if rest:
        new += f"\n{rest}\n"
    new += "\n" + links(["Unreleased", version] + released)
    return new


def main() -> None:
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    text = CHANGELOG.read_text()
    cmd = sys.argv[1]
    if cmd == "check":
        check(text)
        print("CHANGELOG.md is valid")
    elif cmd == "notes" and len(sys.argv) == 3:
        sys.stdout.write(notes(text, sys.argv[2].removeprefix("v")))
    elif cmd == "release" and len(sys.argv) == 3:
        version = sys.argv[2].removeprefix("v")
        new = release(text, version)
        CHANGELOG.write_text(new)
        cargo = CARGO.read_text()
        CARGO.write_text(re.sub(r'(?m)^version = "[^"]+"', f'version = "{version}"', cargo, count=1))
        check(new)
        print(f"CHANGELOG.md and Cargo.toml are at {version}. Commit them and merge to main to release v{version}.")
    else:
        sys.exit(__doc__)


if __name__ == "__main__":
    main()
