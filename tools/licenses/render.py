#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-only
# Copyright (C) 2026 David Nunes-Childs
"""Writes THIRD_PARTY_LICENSES.html (tools/build-licenses.sh runs this; read it first).

It joins what cargo-about found for each program's dependencies (one JSON file per Cargo.toml: the game, the server, the
launchers) into one page that lists every crate once, with its licence's full text once, and adds what is not a crate and
is not the project's own (tools/licenses/other.json: fonts, adapted shader code, lookup tables, recordings). Nothing in the
output depends on the date, the machine or the order of the inputs, so that `tools/build-licenses.sh --check` can compare.

    render.py --config about.toml --gpl LICENSE --other tools/licenses/other.json --source-url URL -o OUT  CRATES.json...

Exit status 1, with the reason on standard error, if a crate's chosen licence is not in about.toml's accepted list, or a
crate that is not the project's own is GPL-3.0-only (it would be compatible, but it is not what the audit expects: look at it).
"""

import argparse
import collections
import html
import json
import os
import re
import sys
import tomllib

# A licence expression's alternatives that are not permissive, noted (not an error) in the summary when a crate offers one
# beside a permissive alternative that is taken instead: "MIT OR Apache-2.0 OR LGPL-2.1-or-later".
COPYLEFT = re.compile(r"^(A|L)?GPL|^MPL|^EPL|^CDDL|^EUPL|^OSL|^CPL|^SSPL|^CC-BY-SA|^MS-RL|^Sleepycat", re.I)
# What a crate's manifest may call its authors, kept to the name: "Ada Lovelace <ada@example.org>" is "Ada Lovelace".
EMAIL = re.compile(r"\s*<[^>]*>")


def text_of(path):
    with open(path, encoding="utf-8") as f:
        return f.read().replace("\r\n", "\n").replace("\r", "\n").strip("\n")


def esc(s):
    return html.escape(s, quote=False)


def attr(s):
    return html.escape(s, quote=True)


HOLDER_LINE = re.compile(r"^\s*(portions\s+)?(copyright|\(c\)|\u00a9)", re.I | re.M)


def says_holder(text):
    """Whether a licence text names its copyright holder (a Copyright line): the permissive licences ask to keep that."""
    return HOLDER_LINE.search(text) is not None


def expression_tokens(expression):
    return [t for t in re.split(r"[\s()/]+|\bOR\b|\bAND\b|\bWITH\b", expression) if t and t not in ("OR", "AND", "WITH")]


def is_own(crate, root):
    """The project's own crates (hotdog-simulator, kitchen-sim, ...): not from a registry, and not vendored (vendor/)."""
    if crate.get("source"):
        return False
    relative = os.path.relpath(crate["manifest_path"], root).replace(os.sep, "/")
    return not relative.startswith("vendor/")


def crate_link(crate, root, first):
    """A crate's entry: its name and version, linked to its page on crates.io. `first`: the anchor for {crate:NAME} links goes here."""
    name, version = crate["name"], crate["version"]
    source = crate.get("source") or ""
    label = f"{esc(name)}&nbsp;{esc(version)}"
    anchor = f' id="c-{attr(name)}-{attr(version)}"' if first else ""
    if source.startswith("registry+"):
        return f'<a{anchor} href="https://crates.io/crates/{attr(name)}/{attr(version)}">{label}</a>'
    relative = os.path.relpath(os.path.dirname(crate["manifest_path"]), root).replace(os.sep, "/")
    if not crate.get("source"):
        return f"<span{anchor}>{label} (a copy in {esc(relative)}/, with one change)</span>"
    return f"<span{anchor}>{label}</span>"


def authors_of(crate):
    names = []
    for author in crate.get("authors") or []:
        name = EMAIL.sub("", author).strip()
        if name and name not in names:
            names.append(name)
    return ", ".join(names)


def collect(paths, root, accepted):
    """The crates and the licence texts of the cargo-about JSON files `paths`."""
    crates = {}  # (name, version, source) -> package
    groups = collections.OrderedDict()  # (licence id, name, text) -> {crates, standard}
    problems = []
    for path in paths:
        with open(path, encoding="utf-8") as f:
            found = json.load(f)
        for entry in found["licenses"]:
            text = entry["text"].replace("\r\n", "\n").replace("\r", "\n").strip("\n")
            for use in entry["used_by"]:
                crate = use["crate"]
                if is_own(crate, root):
                    if entry["id"] != "GPL-3.0-only":
                        problems.append(f'{crate["name"]} (the project\'s own) is {entry["id"]}, not GPL-3.0-only')
                    continue
                if entry["id"] not in accepted:
                    problems.append(f'{crate["name"]} {crate["version"]}: {entry["id"]} is not in about.toml\'s accepted list')
                if entry["id"] == "GPL-3.0-only":
                    problems.append(f'{crate["name"]} {crate["version"]} is GPL-3.0-only and is not the project\'s own: look at it')
                key = (crate["name"], crate["version"], crate.get("source") or "")
                crates[key] = crate
                group = groups.setdefault((entry["id"], entry["name"], text), {"crates": {}, "standard": False})
                group["crates"][key] = crate
                # cargo-about gives a crate whose package has no licence file the licence's standard text (no source file).
                group["standard"] = group["standard"] or entry.get("source_path") is None
    return crates, groups, problems


def render(args):
    root = os.getcwd()
    with open(args.config, "rb") as f:
        accepted = set(tomllib.load(f)["accepted"])
    crates, groups, problems = collect(args.crates, root, accepted)
    if problems:
        for problem in sorted(set(problems)):
            print(f"licences: {problem}", file=sys.stderr)
        sys.exit(1)

    with open(args.other, encoding="utf-8") as f:
        other = json.load(f)
    # The project's copyright line, as COPYRIGHT has it (the one place it is written).
    holder = next((line.strip() for line in text_of(args.copyright).split("\n") if line.startswith("Copyright")), "")
    if not holder:
        raise SystemExit("licences: no Copyright line in " + args.copyright)

    out = []
    w = out.append
    w("<!doctype html>")
    w('<html lang="en">')
    w("<head>")
    w('<meta charset="utf-8">')
    w('<meta name="viewport" content="width=device-width,initial-scale=1">')
    w("<title>Hotdog Simulator licences</title>")
    w(STYLE)
    w("</head>")
    w("<body>")
    w("<main>")
    w("<h1>Hotdog Simulator: licences</h1>")
    w(
        "<p>This page is the licence notice of the game: the licence of the game itself, the licences of everything that is not "
        "the project's own and is part of a build of it, and where the source code is. It comes with every package and with the "
        "browser game, and it is generated (<code>tools/build-licenses.sh</code>), not written by hand, from what the builds "
        "contain.</p>"
    )
    w('<nav aria-label="Contents"><ol>')
    w('<li><a href="#game">The game: GPL-3.0-only, and its source code</a></li>')
    w('<li><a href="#other">Fonts, shader code, data tables and recordings that are not the project\'s own</a></li>')
    w('<li><a href="#crates">Rust crates</a> (%d of them) <ul>' % len(crates))
    w('<li><a href="#crate-summary">Which licences, and how many crates</a></li>')
    w('<li><a href="#crate-texts">The licence texts, and the crates each covers</a></li>')
    w("</ul></li>")
    w('<li><a href="#gpl">The GNU General Public License, version 3</a></li>')
    w("</ol></nav>")

    # 1. The game.
    source = args.source_url.strip()
    w('<h2 id="game">1. The game: GPL-3.0-only, and its source code</h2>')
    w(
        f"<p>Hotdog Simulator, {esc(holder)}, is free software: you can redistribute it and modify it "
        "under the terms of the <strong>GNU General Public License, version 3 only</strong> (GPL-3.0-only), the text of which is "
        '<a href="#gpl">at the end of this page</a> and in the file <code>LICENSE</code>. It is distributed in the hope that it '
        "will be useful, but <strong>without any warranty</strong>; without even the implied warranty of merchantability or "
        "fitness for a particular purpose. This covers the project's own work: its source code (the game, the server, the "
        "launchers, the map editor and the scripts that build the packages), its shaders, maps, procedurally made models and "
        "textures, and its synthesised sounds. It does not cover the material of section 2 and the crates of section 3, which "
        "keep their own licences, listed here.</p>"
    )
    if source:
        w(
            "<p><strong>The source code.</strong> The complete corresponding source of the game, the server, the launchers and the "
            f'browser build, and the scripts to build them, is published at <a href="{attr(source)}">{esc(source)}</a>. Each build '
            "names the commit it was made from: the part of its version number after the <code>+</code> (for example "
            "<code>0.1.0+1a2b3c4</code>, in <code>HOW-TO-RUN.txt</code> and in the first lines of the game's log; the hosted page "
            "names its release, which ends with that version, in <code>assets/release.txt</code>). Get that version with <code>git clone</code> and "
            "<code>git checkout</code> of that commit.</p>"
        )
    else:
        w(
            "<p><strong>The source code</strong> is published in the project's public repository (its address is the "
            "<code>repository</code> line of <code>Cargo.toml</code>).</p>"
        )
    w(
        "<p>Every package of the game carries <code>LICENSE</code>, <code>COPYRIGHT</code>, <code>THIRD_PARTY_NOTICES.md</code> "
        "and this page.</p>"
    )

    # 2. Not the project's own, not a crate.
    w('<h2 id="other">2. Fonts, shader code, data tables and recordings that are not the project\'s own</h2>')
    w("<p>" + esc(other["intro"]) + "</p>")
    for item in other["items"]:
        w(f'<section class="item" id="{attr(item["id"])}">')
        w(f'<h3>{esc(item["title"])}</h3>')
        for paragraph in item["paragraphs"]:
            w("<p>" + paragraph_html(paragraph, crates) + "</p>")
        if item.get("rows"):
            w('<table class="rows">')
            w("<thead><tr>" + "".join(f"<th>{esc(h)}</th>" for h in item["rows"]["head"]) + "</tr></thead><tbody>")
            for row in item["rows"]["body"]:
                w("<tr>" + "".join(f"<td>{esc(c)}</td>" for c in row) + "</tr>")
            w("</tbody></table>")
        for name in item.get("texts", []):
            w(f'<details><summary>{esc(name.get("label") or os.path.basename(name["file"]))}</summary><pre>{esc(text_of(name["file"]))}</pre></details>')
        w("</section>")

    # 3. The crates.
    expressions = collections.Counter(" ".join(c["license"].split()) for c in crates.values())
    chosen = collections.Counter()
    for (lid, _name, _text), group in groups.items():
        chosen[lid] += len(group["crates"])
    w(f'<h2 id="crates">3. Rust crates ({len(crates)})</h2>')
    w(
        "<p>The programs are written in Rust, and are built from the crates below (the libraries from crates.io, and wgpu-hal, "
        "which is a copy in the project's <code>vendor/</code> folder). The list is what the game for Linux, Windows, macOS and "
        "the browser (both builds), the server and the launchers need, as <code>cargo-about</code> found it from "
        "<code>Cargo.lock</code>, for every feature the game has: a crate is listed once for each version in use. Libraries that "
        "only build or test the programs (build scripts, test helpers) are left out; macro crates, which only run while the "
        "programs are compiled, are listed all the same.</p>"
    )
    w(
        "<p>Where a crate offers its licence as a choice (<code>MIT OR Apache-2.0</code>), the choice taken here is the first "
        "alternative the project accepts, and that licence's text is the one listed. All of them are permissive licences "
        "that GPL-3.0-only allows the project to combine and redistribute with its own code.</p>"
    )
    w('<h3 id="crate-summary">Which licences, and how many crates</h3>')
    w('<table class="rows"><thead><tr><th>Licence taken</th><th>Crates</th></tr></thead><tbody>')
    for lid, count in sorted(chosen.items(), key=lambda kv: (-kv[1], kv[0])):
        w(f"<tr><td>{esc(lid)}</td><td>{count}</td></tr>")
    w("</tbody></table>")
    w('<table class="rows"><thead><tr><th>Licence as the crate publishes it</th><th>Crates</th><th>Note</th></tr></thead><tbody>')
    for expression, count in sorted(expressions.items(), key=lambda kv: (-kv[1], kv[0])):
        copyleft = [t for t in expression_tokens(expression) if COPYLEFT.match(t)]
        note = "a copyleft alternative is offered; a permissive one is taken instead" if copyleft else ""
        w(f"<tr><td>{esc(expression)}</td><td>{count}</td><td>{esc(note)}</td></tr>")
    w("</tbody></table>")
    w(
        "<p>No crate is licensed only under a copyleft or an unknown licence. A crate under two licences at once "
        "(<code>A AND B</code>) is counted under each.</p>"
    )
    w('<h3 id="crate-texts">The licence texts, and the crates each covers</h3>')
    w(
        "<p>Each text is printed once, with every crate it covers. Where a crate's package carries no licence file, the "
        "licence's standard text stands in for it; where the text a crate carries names no copyright holder, the holders "
        "are taken to be the authors its manifest names. Either way they are shown after the crate's name.</p>"
    )
    anchored = set()
    order = sorted(groups.items(), key=lambda kv: (-chosen[kv[0][0]], kv[0][0], -len(kv[1]["crates"]), kv[0][2]))
    for number, ((lid, name, text), group) in enumerate(order, 1):
        members = sorted(group["crates"].values(), key=lambda c: (c["name"].lower(), c["version"]))
        w(f'<article class="lic" id="lic-{number}">')
        kind = "standard text" if group["standard"] else "text from the crate"
        w(f'<h4>{esc(name)} <small>({esc(lid)}; {len(members)} crate{"s" if len(members) != 1 else ""}; {kind})</small></h4>')
        listing = []
        holders = group["standard"] or (lid != "Apache-2.0" and not says_holder(text))
        for crate in members:
            key = (crate["name"], crate["version"])
            line = crate_link(crate, root, key not in anchored)
            anchored.add(key)
            if holders:
                names = authors_of(crate)
                if names:
                    line += f' <span class="by">({esc(names)})</span>'
            listing.append(line)
        w('<p class="used">' + ", ".join(listing) + "</p>")
        w(f"<pre>{esc(text)}</pre>")
        w("</article>")

    # 4. The GPL.
    w('<h2 id="gpl">4. The GNU General Public License, version 3</h2>')
    w("<p>The licence of the game itself (section 1), as in the file <code>LICENSE</code>.</p>")
    w(f"<pre>{esc(text_of(args.gpl))}</pre>")
    w("</main>")
    w("</body>")
    w("</html>")

    with open(args.output, "w", encoding="utf-8", newline="\n") as f:
        f.write("\n".join(out) + "\n")

    # What the build script prints.
    print(f"licences: {len(crates)} crates (name and version), {len(groups)} licence texts, {len(other['items'])} other items")
    print("licences: licence taken:")
    for lid, count in sorted(chosen.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"licences:   {count:4d}  {lid}")
    print("licences: licence expression as published:")
    for expression, count in sorted(expressions.items(), key=lambda kv: (-kv[1], kv[0])):
        print(f"licences:   {count:4d}  {expression}")


def paragraph_html(paragraph, crates):
    """A paragraph of other.json: plain text (escaped), with [text](url) links and {crate:NAME} links to the crate's entry."""
    s = esc(paragraph)
    s = re.sub(r"\[([^\]]+)\]\((https?://[^)\s]+|#[A-Za-z0-9_.-]+)\)", lambda m: f'<a href="{m.group(2)}">{m.group(1)}</a>', s)

    def crate(match):
        name = match.group(1)
        versions = sorted(v for (n, v, _s) in crates if n == name)
        if not versions:
            raise SystemExit(f"licences: other.json names the crate {name}, which is not among the crates")
        return f'<a href="#c-{name}-{versions[-1]}">{name}</a>'

    return re.sub(r"\{crate:([A-Za-z0-9_-]+)\}", crate, s)


STYLE = """<style>
:root{color-scheme:light dark;--bg:#fbfaf6;--fg:#1d2321;--dim:#5d6b66;--rule:#d6d9d2;--code:#eef0ea;--accent:#8a5a00}
@media (prefers-color-scheme:dark){:root:not([data-theme=light]){--bg:#101515;--fg:#e8e6da;--dim:#9eafa5;--rule:#2b3a36;--code:#172120;--accent:#dfb97b}}
:root[data-theme=dark]{--bg:#101515;--fg:#e8e6da;--dim:#9eafa5;--rule:#2b3a36;--code:#172120;--accent:#dfb97b}
html{background:var(--bg)}
body{margin:0;background:var(--bg);color:var(--fg);font:16px/1.55 system-ui,sans-serif}
main{max-width:56rem;margin:0 auto;padding:24px 16px 64px}
h1{font-size:1.7rem;margin:.2em 0 .6em}h2{font-size:1.3rem;margin:2.2em 0 .5em;padding-top:.6em;border-top:1px solid var(--rule)}
h3{font-size:1.08rem;margin:1.6em 0 .4em}h4{font-size:.98rem;margin:0 0 .4em}h4 small{font-weight:400;color:var(--dim)}
a{color:var(--accent)}code,pre{font:13px/1.45 ui-monospace,monospace}code{background:var(--code);padding:0 .25em;border-radius:3px}
pre{background:var(--code);padding:12px 14px;border-radius:6px;overflow:auto;white-space:pre-wrap;overflow-wrap:anywhere;margin:.5em 0 0}
nav ol{margin:.4em 0;padding-left:1.4em}nav ul{margin:.1em 0}
.lic{margin:1.4em 0;padding-top:1em;border-top:1px solid var(--rule)}.used{margin:.2em 0 .5em;font-size:.9rem;color:var(--dim)}.used a{color:var(--fg)}.by{color:var(--dim)}
.item{margin:1.4em 0}.item h3{margin-top:0}details{margin:.5em 0}summary{cursor:pointer;color:var(--accent)}
table.rows{border-collapse:collapse;margin:.8em 0;font-size:.92rem}.rows th,.rows td{text-align:left;padding:3px 14px 3px 0;border-bottom:1px solid var(--rule);vertical-align:top}
</style>"""


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--config", required=True, help="about.toml (its accepted list)")
    parser.add_argument("--gpl", required=True, help="the file with the GPL's text (LICENSE)")
    parser.add_argument("--other", required=True, help="tools/licenses/other.json")
    parser.add_argument("--copyright", required=True, help="the file with the project's Copyright line (COPYRIGHT)")
    parser.add_argument("--source-url", default="", help="where the source code is published")
    parser.add_argument("-o", "--output", required=True)
    parser.add_argument("crates", nargs="+", help="the JSON files of `cargo about generate --format json`")
    render(parser.parse_args())


if __name__ == "__main__":
    main()
