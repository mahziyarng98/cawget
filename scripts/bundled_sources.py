#!/usr/bin/env python3
"""Corresponding Source for the copyleft libraries bundled in a release AppImage (Ubuntu 22.04 builds).

Reads packaging/bundled-sources/<tag>.tsv (source package, exact version, class, ...) and, on Ubuntu 22.04
with deb-src enabled:
  fetch     apt-get source --download-only <package>=<version> for every "copyleft" or "unknown" package
            (apt checks each file against the signed archive index)
  verify    checks every file against the sizes and SHA-256 sums in its .dsc
  classify  decides the "unknown" packages from their debian/copyright (included if copyleft)
  pack      writes README.md and <name>.tar.zst (deterministic tar, zstd)
Usage: bundled_sources.py all <list.tsv> <workdir> <archive-name>   (runs the four steps in order)
Standard library only (Python 3.10).
"""

import hashlib
import json
import re
import subprocess
import sys
import tarfile
from pathlib import Path

COPYLEFT = re.compile(
    r"\b(L?GPL|AGPL|MPL|EPL|CDDL|EUPL)\b|GNU (Lesser |Library )?General Public License|Mozilla Public License",
    re.I,
)


def read_list(path: Path) -> list[dict]:
    rows = []
    for line in path.read_text().splitlines():
        if not line or line.startswith("#"):
            continue
        src, ver, cls, lic, bins, libs = line.split("\t")
        rows.append({"source": src, "version": ver, "class": cls, "licences": lic,
                     "binaries": bins.split(","), "libraries": libs.split(",")})
    return rows


def no_epoch(version: str) -> str:
    return version.split(":", 1)[1] if ":" in version else version


def dsc_files(dsc: Path) -> list[tuple[str, int, str]]:
    """(name, size, sha256) of every file listed in the .dsc."""
    files, section = [], None
    for line in dsc.read_text(errors="replace").splitlines():
        if line.startswith("Checksums-Sha256:"):
            section = True
            continue
        if section and line.startswith(" "):
            sha, size, name = line.split()
            files.append((name, int(size), sha))
        elif section:
            break
    if not files:
        sys.exit(f"{dsc.name}: no Checksums-Sha256 field")
    return files


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def fetch(rows: list[dict], work: Path) -> None:
    failed = []
    for r in rows:
        if r["class"] == "permissive":
            continue
        d = work / f"{r['source']}_{no_epoch(r['version'])}"
        d.mkdir(parents=True, exist_ok=True)
        cmd = ["apt-get", "source", "--download-only", "--only-source", f"{r['source']}={r['version']}"]
        if subprocess.run(cmd, cwd=d).returncode != 0:
            failed.append(f"{r['source']}={r['version']}")
    if failed:
        sys.exit("could not fetch: " + ", ".join(failed))


def verify(rows: list[dict], work: Path) -> None:
    for r in rows:
        if r["class"] == "permissive":
            continue
        d = work / f"{r['source']}_{no_epoch(r['version'])}"
        dscs = list(d.glob("*.dsc"))
        if len(dscs) != 1:
            sys.exit(f"{d.name}: expected one .dsc, found {len(dscs)}")
        r["dsc"] = dscs[0].name
        r["files"] = []
        for name, size, sha in dsc_files(dscs[0]):
            f = d / name
            if not f.is_file() or f.stat().st_size != size or sha256(f) != sha:
                sys.exit(f"{d.name}/{name}: missing or does not match its .dsc")
            r["files"].append((name, size, sha))
        r["files"].append((dscs[0].name, dscs[0].stat().st_size, sha256(dscs[0])))
        print(f"verified {d.name}: {len(r['files']) - 1} file(s) match {dscs[0].name}")


def copyright_text(d: Path) -> str:
    for f in sorted(d.iterdir()):
        if ".debian.tar." in f.name or (".tar." in f.name and ".orig" not in f.name and not f.name.endswith(".asc")):
            with tarfile.open(f) as t:
                for m in t.getmembers():
                    if re.search(r"(^|/)debian/copyright$", m.name):
                        return t.extractfile(m).read().decode(errors="replace")
    return ""


def classify(rows: list[dict], work: Path) -> None:
    for r in rows:
        if r["class"] != "unknown":
            continue
        d = work / f"{r['source']}_{no_epoch(r['version'])}"
        text = copyright_text(d)
        if not text:
            sys.exit(f"{d.name}: no debian/copyright found")
        hits = sorted({m.group(0) for m in COPYLEFT.finditer(text)})
        r["class"] = "copyleft" if hits else "permissive"
        r["licences"] = "; ".join(sorted({x.strip() for x in re.findall(r"^License:\s*(.+)$", text, re.M)})[:8])
        r["decided"] = "debian/copyright: " + (", ".join(hits[:4]) if hits else "no copyleft licence named")
        print(f"classified {d.name}: {r['class']} ({r['decided']})")


def pack(rows: list[dict], work: Path, name: str) -> None:
    inc = [r for r in rows if r["class"] == "copyleft"]
    exc = [r for r in rows if r["class"] == "permissive"]
    out = [
        f"# Corresponding Source: {name}",
        "",
        "Complete source of the copyleft (GPL, LGPL, MPL, ...) libraries bundled in the matching Cawget AppImage.",
        "They are unmodified Ubuntu 22.04 packages; each directory holds the Debian source package exactly as",
        "published by Ubuntu (.dsc, upstream tarball(s) and Debian packaging). Build one with",
        "`dpkg-source -x <name>.dsc` and `dpkg-buildpackage`. Every file was checked against the SHA-256 sums",
        "in its .dsc, which apt verified against Ubuntu's signed archive index.",
        "",
        "Packages are included when any of their copyright files names a copyleft licence, also when it covers",
        "only part of the package or is one option of a dual licence.",
        "",
        f"## Included ({len(inc)} source packages)",
        "",
        "| Source package | Version | Libraries in the AppImage |",
        "|---|---|---|",
    ]
    out += [f"| {r['source']} | {r['version']} | {', '.join(r['libraries'])} |" for r in inc]
    out += ["", "### Files and SHA-256", ""]
    for r in inc:
        out.append(f"{r['source']}_{no_epoch(r['version'])}/")
        out += [f"  {sha}  {n}  ({size} bytes)" for n, size, sha in r["files"]]
    out += ["", f"## Not included ({len(exc)} permissive source packages)", "",
            "Their licences (MIT, BSD, X11, Apache-2.0 and similar) do not require distributing source code.",
            "They are still listed in the release's bundled-libraries.txt with their exact versions.", "",
            "| Source package | Version | Licence(s) | Reason |", "|---|---|---|---|"]
    for r in exc:
        reason = r.get("decided", "copyright files in the AppImage name no copyleft licence")
        out.append(f"| {r['source']} | {r['version']} | {r['licences'] or 'see copyright file'} | {reason} |")
    root = work / "pack" / name.removesuffix(".tar.zst")
    root.mkdir(parents=True)
    (root / "README.md").write_text("\n".join(out) + "\n")
    for r in inc:
        src = work / f"{r['source']}_{no_epoch(r['version'])}"
        dst = root / src.name
        dst.mkdir()
        for n, _, _ in r["files"]:
            (dst / n).hardlink_to(src / n)
    (work / "summary.json").write_text(json.dumps({"included": [r["source"] for r in inc],
                                                   "excluded": [r["source"] for r in exc]}, indent=1))
    tar = ["tar", "--sort=name", "--mtime=@0", "--owner=0", "--group=0", "--numeric-owner", "-C", str(root.parent),
           "-cf", "-", root.name]
    with open(work / name, "wb") as f:
        p = subprocess.Popen(tar, stdout=subprocess.PIPE)
        subprocess.run(["zstd", "-q", "-19", "-T0", "-c"], stdin=p.stdout, stdout=f, check=True)
        if p.wait() != 0:
            sys.exit("tar failed")
    print(f"packed {name}: {len(inc)} included, {len(exc)} excluded, {(work / name).stat().st_size} bytes")


def main() -> None:
    if len(sys.argv) != 5 or sys.argv[1] != "all":
        sys.exit(__doc__)
    rows, work, name = read_list(Path(sys.argv[2])), Path(sys.argv[3]).resolve(), sys.argv[4]
    work.mkdir(parents=True, exist_ok=True)
    fetch(rows, work)
    verify(rows, work)
    classify(rows, work)
    pack(rows, work, name)


if __name__ == "__main__":
    main()
