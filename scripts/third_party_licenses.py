#!/usr/bin/env python3
"""Generates THIRD_PARTY_LICENSES.md: the licences of everything compiled into the shipped programs.

* Rust: the normal (non-dev, non-build) dependencies of the `cawget` and `cawget-cli` binaries on
  x86_64 Linux, from `cargo metadata --locked --offline`; licence texts from each crate's own files.
* npm: the production dependencies of gui/package.json plus the Svelte runtime, which the compiler
  bundles into the frontend; licence texts from gui/node_modules (run `npm ci` first).

Every licence expression is checked for compatibility with GPL-3.0-or-later; the script fails otherwise.
Usage: python3 scripts/third_party_licenses.py [--check]   (--check: fail if the file is out of date)
Standard library only.
"""

import json
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "THIRD_PARTY_LICENSES.md"
BINARIES = ("cawget", "cawget-cli")
OWN = {"cawget", "cawget-cli", "cawget-core", "cawget-testserver"}
# Licences that may be combined with GPL-3.0-or-later (FSF list; Apache-2.0 is compatible with GPLv3).
COMPATIBLE = {
    "MIT", "MIT-0", "Apache-2.0", "Apache-2.0 WITH LLVM-exception", "BSD-2-Clause", "BSD-3-Clause", "ISC",
    "Zlib", "0BSD", "Unicode-3.0", "Unicode-DFS-2016", "MPL-2.0", "Unlicense", "CC0-1.0", "BSL-1.0",
    "LGPL-2.1-or-later", "LGPL-3.0-or-later", "GPL-3.0-or-later", "GPL-2.0-or-later",
}
# Standard texts for the short licences, for packages that ship no licence file of their own.
STANDARD = {
    "MIT": """Permission is hereby granted, free of charge, to any person obtaining a copy of this software and
associated documentation files (the "Software"), to deal in the Software without restriction, including
without limitation the rights to use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
of the Software, and to permit persons to whom the Software is furnished to do so, subject to the following
conditions:

The above copyright notice and this permission notice shall be included in all copies or substantial portions
of the Software.

THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION OF
CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
DEALINGS IN THE SOFTWARE.""",
    "BSD-3-Clause": """Redistribution and use in source and binary forms, with or without modification, are permitted provided
that the following conditions are met:

1. Redistributions of source code must retain the above copyright notice, this list of conditions and the
   following disclaimer.
2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
   following disclaimer in the documentation and/or other materials provided with the distribution.
3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or
   promote products derived from this software without specific prior written permission.

THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED
WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A
PARTICULAR PURPOSE ARE DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY
DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO,
PROCUREMENT OF SUBSTITUTE GOODS OR SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR
OTHERWISE) ARISING IN ANY WAY OUT OF THE USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH
DAMAGE.""",
}
LICENSE_FILE = re.compile(r"^(LICEN[CS]E|COPYING|NOTICE|UNLICENSE|COPYRIGHT)([-_.].*)?$", re.I)


def spdx_ok(expr: str) -> bool:
    """True if some choice in the SPDX expression is entirely made of compatible licences."""
    expr = expr.replace("/", " OR ")
    tokens = re.findall(r"\(|\)|\bAND\b|\bOR\b|[A-Za-z0-9.+-]+(?: WITH [A-Za-z0-9.+-]+)?", expr)
    pos = 0

    def parse_or():
        nonlocal pos
        v = parse_and()
        while pos < len(tokens) and tokens[pos] == "OR":
            pos += 1
            v = parse_and() or v
        return v

    def parse_and():
        nonlocal pos
        v = parse_atom()
        while pos < len(tokens) and tokens[pos] == "AND":
            pos += 1
            v = parse_atom() and v
        return v

    def parse_atom():
        nonlocal pos
        t = tokens[pos]
        pos += 1
        if t == "(":
            v = parse_or()
            pos += 1  # ")"
            return v
        return t in COMPATIBLE

    ok = parse_or()
    return ok and pos == len(tokens)


def license_texts(directory: Path) -> list[tuple[str, str]]:
    found = []
    for f in sorted(directory.iterdir()):
        if f.is_file() and LICENSE_FILE.match(f.name):
            found.append((f.name, f.read_text(errors="replace").strip()))
    return found


def rust_packages() -> list[dict]:
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--locked", "--offline", "--format-version", "1",
         "--filter-platform", "x86_64-unknown-linux-gnu"],
        cwd=ROOT, check=True, capture_output=True, text=True).stdout)
    pkgs = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    todo = [p["id"] for p in meta["packages"] if p["name"] in BINARIES]
    seen = set()
    while todo:
        i = todo.pop()
        if i in seen:
            continue
        seen.add(i)
        for d in nodes[i]["deps"]:
            if any(k["kind"] is None for k in d["dep_kinds"]):
                todo.append(d["pkg"])
    out = []
    for i in seen:
        p = pkgs[i]
        if p["name"] in OWN:
            continue
        out.append({
            "name": p["name"], "version": p["version"],
            "license": p["license"] or f"see {p['license_file']}",
            "texts": license_texts(Path(p["manifest_path"]).parent),
        })
    return sorted(out, key=lambda p: (p["name"], p["version"]))


def npm_packages() -> list[dict]:
    gui = ROOT / "gui"
    names = sorted(json.loads((gui / "package.json").read_text()).get("dependencies", {})) + ["svelte"]
    out = []
    for n in names:
        d = gui / "node_modules" / n
        if not d.is_dir():
            sys.exit(f"{d} is missing: run `npm ci` in gui/ first")
        pj = json.loads((d / "package.json").read_text())
        out.append({"name": n, "version": pj["version"], "license": pj.get("license", "UNKNOWN"), "texts": license_texts(d)})
    return out


def render(rust: list[dict], npm: list[dict]) -> str:
    lines = [
        "# Third-party licences",
        "",
        "Cawget is licensed under GPL-3.0-or-later (see LICENSE). It includes the components below, each under its",
        "own licence. This file is generated by `scripts/third_party_licenses.py`; do not edit it by hand.",
        "",
        "All licences listed here are compatible with GPL-3.0-or-later. Libraries that the AppImage bundles from the",
        "build system (GTK, WebKitGTK and their dependencies) are listed in docs/BUNDLED_LIBRARIES.md.",
        "",
        "## Summary",
        "",
        f"* Rust crates compiled into `cawget` and `cawget-cli`: {len(rust)}",
        f"* npm packages compiled into the desktop app's interface: {len(npm)}",
        "",
        "| Component | Version | Licence |",
        "|---|---|---|",
    ]
    for kind, items in (("npm", npm), ("crate", rust)):
        for p in items:
            lines.append(f"| {p['name']} ({kind}) | {p['version']} | {p['license']} |")
    # Licence texts, each distinct text once, with the components it applies to.
    texts: dict[str, list[str]] = {}
    missing = []
    for kind, items in (("npm", npm), ("crate", rust)):
        for p in items:
            if not p["texts"]:
                missing.append(p)
            for _, t in p["texts"]:
                texts.setdefault(t, []).append(f"{p['name']} {p['version']}")
    lines += ["", "## Licence texts", ""]
    for n, (t, users) in enumerate(texts.items(), 1):
        lines += [f"### Text {n}", "", "Applies to: " + ", ".join(users), "", "```text", t.replace("```", "'''"), "```", ""]
    if missing:
        # Where another component carries the full text of the same licence, point to it.
        full = {}
        for n, (t, _) in enumerate(texts.items(), 1):
            if "Mozilla Public License Version 2.0" in t:
                full.setdefault("MPL-2.0", n)
            if "Apache License" in t and "Version 2.0, January 2004" in t:
                full.setdefault("Apache-2.0", n)
        lines += ["## Components without a licence file", "",
                  "These packages state their licence only in their metadata; the standard text of that licence applies.", ""]
        for p in missing:
            ids = re.findall(r"[A-Za-z0-9.-]+", p["license"].replace(" OR ", " ").replace(" AND ", " "))
            refs = [f"{i}: Text {full[i]}" if i in full else f"{i}: standard text below" for i in ids if i in full or i in STANDARD]
            lines.append(f"* {p['name']} {p['version']}: {p['license']}" + (f" ({'; '.join(refs)})" if refs else ""))
        lines.append("")
        for i in sorted({i for p in missing for i in STANDARD if i in p["license"]}):
            lines += [f"### Standard {i} text", "", "```text", STANDARD[i], "```", ""]
    return "\n".join(lines)


def main() -> None:
    rust, npm = rust_packages(), npm_packages()
    bad = [f"{p['name']} {p['version']}: {p['license']}" for p in rust + npm if not spdx_ok(p["license"])]
    if bad:
        sys.exit("licences not known to be compatible with GPL-3.0-or-later:\n  " + "\n  ".join(bad))
    text = render(rust, npm)
    if "--check" in sys.argv:
        if not OUT.exists() or OUT.read_text() != text:
            sys.exit(f"{OUT.name} is out of date: run python3 scripts/third_party_licenses.py")
        print(f"{OUT.name} is up to date ({len(rust)} crates, {len(npm)} npm packages)")
        return
    OUT.write_text(text)
    print(f"wrote {OUT.name}: {len(rust)} crates, {len(npm)} npm packages, all GPL-3.0-or-later compatible")


if __name__ == "__main__":
    main()
