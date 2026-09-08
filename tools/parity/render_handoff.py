#!/usr/bin/env python3
"""Render the checked-in parity guide as a deterministic, offline HTML file.

Install requirements-docs.txt into an isolated development environment first.
Use --check to validate links and verify that index.html is up to date.
"""

import argparse
import html
from html.parser import HTMLParser
from pathlib import Path
import re
from urllib.parse import unquote, urlsplit

import markdown


ROOT = Path(__file__).resolve().parents[2]
DOCS = ROOT / "docs" / "parity"
OUTPUT = DOCS / "index.html"
SECTIONS = [
    ("overview", "README.md", "Audit & gaps"),
    ("architecture", "ARCHITECTURE.md", "Architecture"),
    ("roadmap", "IMPLEMENTATION_PLAN.md", "Implementation plan"),
    ("conformance", "CONFORMANCE.md", "Tests & acceptance"),
    ("handoff", "AGENT_HANDOFF.md", "Coding-agent brief"),
]
TARGETS = {filename: key for key, filename, _ in SECTIONS}

CSS = """
:root { color-scheme: light; --ink:#172a35; --muted:#526675; --line:#d3dfe3;
  --accent:#007367; --paper:#fff; --wash:#f2f6f7; }
* { box-sizing:border-box; }
html { scroll-behavior:smooth; scroll-padding-top:24px; }
body { margin:0; color:var(--ink); background:var(--wash);
  font:16px/1.65 system-ui,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif; }
a { color:var(--accent); text-decoration-thickness:1px; text-underline-offset:3px; }
a:hover { color:#004c45; }
a:focus-visible { outline:3px solid #c56f05; outline-offset:4px; }
.shell { display:grid; grid-template-columns:220px minmax(0,1000px); gap:32px;
  max-width:1332px; margin:auto; padding:32px; }
nav { position:sticky; top:32px; align-self:start; }
.brand { font-size:20px; font-weight:750; letter-spacing:-.6px; }
.kicker { font-size:12px; text-transform:uppercase; letter-spacing:1.3px;
  font-weight:700; color:var(--accent); margin:0 0 12px; }
nav ul { list-style:none; padding:0; margin:24px 0; }
nav li { margin:6px 0; }
nav a { display:block; padding:9px 12px; border-radius:6px; text-decoration:none; }
nav a:hover { background:#e1efeb; }
.note { color:var(--muted); font-size:13px; }
main { min-width:0; }
.hero, article { background:var(--paper); border:1px solid var(--line);
  border-radius:12px; padding:36px; margin-bottom:24px; }
.hero h1 { font-size:clamp(28px,4vw,46px); line-height:1.14; letter-spacing:-1.5px;
  margin:12px 0 20px; max-width:800px; }
.hero p { max-width:750px; }
.summary { padding:16px 20px; border-left:4px solid var(--accent);
  background:#f0f8f5; border-radius:0 6px 6px 0; }
.facts { display:grid; grid-template-columns:repeat(3,1fr); gap:14px; margin-top:24px; }
.fact { border-top:1px solid var(--line); padding-top:14px; }
.fact strong { display:block; font-size:28px; line-height:1.3; }
.fact span { color:var(--muted); font-size:13px; }
article > h1 { font-size:29px; line-height:1.3; margin:8px 0 24px; letter-spacing:-.6px; }
h2 { font-size:22px; line-height:1.4; margin:36px 0 16px; }
h3 { font-size:18px; margin:28px 0 12px; }
p { margin:14px 0; }
li { margin:8px 0; }
code { font-family:ui-monospace,SFMono-Regular,Consolas,monospace; font-size:.87em;
  overflow-wrap:anywhere; background:#eef3f5; padding:2px 4px; border-radius:3px; }
pre { white-space:pre; overflow:auto; border:1px solid var(--line); padding:20px;
  background:#f5f8fa; border-radius:7px; line-height:1.5; max-width:100%; }
pre code { overflow-wrap:normal; padding:0; background:none; font-size:12px; }
.table-wrap { overflow:auto; margin:20px 0; }
table { width:100%; border-collapse:collapse; font-size:14px; }
th,td { text-align:left; vertical-align:top; border-bottom:1px solid var(--line);
  padding:12px 10px; min-width:100px; }
th { background:#eef4f3; font-weight:650; }
tr:last-child td { border-bottom:0; }
blockquote { margin:20px 0; border-left:4px solid var(--accent); padding:1px 20px; }
.source { font-size:12px; color:var(--muted); }
footer { color:var(--muted); font-size:13px; padding:8px 0 40px; }
@media(max-width:950px) {
  .shell { grid-template-columns:minmax(0,1fr); padding:20px; gap:20px; }
  nav { position:static; }
  nav ul { display:flex; flex-wrap:wrap; margin:12px 0 0; gap:4px; }
  nav li { margin:0; }
  nav a { padding:6px 9px; }
  nav .note { display:none; }
}
@media(max-width:580px) {
  .shell { padding:12px; }
  .hero,article { padding:22px 18px; border-radius:8px; }
  .facts { grid-template-columns:1fr; }
  article > h1 { font-size:25px; }
  pre { padding:12px; }
}
@media(prefers-reduced-motion:reduce) { html { scroll-behavior:auto; } }
@media print {
  body { background:white; font-size:11pt; }
  .shell { display:block; padding:0; }
  nav { display:none; }
  .hero,article { border:0; padding:0; border-radius:0; }
  article { break-before:page; }
  pre { white-space:pre-wrap; }
  .table-wrap { overflow:visible; }
  th,td { min-width:0; }
}
"""


def render_section(key, filename):
    source = (DOCS / filename).read_text(encoding="utf-8")
    body = markdown.markdown(source, extensions=["fenced_code", "tables", "toc"])
    body = re.sub(r'id="([^"]+)"', lambda m: f'id="{key}-{m[1]}"', body)

    def link(match):
        url = html.unescape(match[1])
        parts = urlsplit(url)
        if not parts.scheme and not parts.netloc:
            if parts.path in TARGETS:
                section = TARGETS[parts.path]
                fragment = f"{section}-{parts.fragment}" if parts.fragment else section
                return f'href="#{html.escape(fragment, quote=True)}"'
            if not parts.path and parts.fragment:
                return f'href="#{key}-{html.escape(parts.fragment, quote=True)}"'
            if parts.path == "index.html":
                return 'href="#top"'
        return match[0]

    body = re.sub(r'href="([^"]+)"', link, body)
    body = body.replace("<table>", '<div class="table-wrap"><table>')
    body = body.replace("</table>", "</table></div>")
    return (f'<article id="{key}" aria-label="{html.escape(filename)}">\n'
            f'<p class="source">Source: <a href="{filename}">{filename}</a></p>\n'
            f'{body}\n</article>')


def render():
    navigation = "\n".join(f'<li><a href="#{key}">{label}</a></li>'
                           for key, _, label in SECTIONS)
    sections = "\n".join(render_section(key, filename) for key, filename, _ in SECTIONS)
    return f'''<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<meta name="description" content="Audited Python parity gaps, architecture, implementation roadmap and coding-agent handoff for rhino3dm-rs.">
<title>rhino3dm-rs · Python parity implementation guide</title>
<style>{CSS}</style>
</head>
<body id="top">
<div class="shell">
<nav aria-label="Guide sections">
<div class="brand">rhino3dm-rs</div>
<div class="note">Python parity · engineering brief</div>
<ul>{navigation}</ul>
<p class="note">Audited 7 September 2026<br>Baseline 34a8846<br>Offline reading copy</p>
<a href="oracle-lock.json">Oracle artifact lock ↗</a>
<a href="python-api-8.32.1.json">Primary API inventory ↗</a>
</nav>
<main>
<header class="hero">
<p class="kicker">Audit → architecture → implementation</p>
<h1>A testable path to Python parity in pure Rust.</h1>
<p>The existing package is a useful archive and scene foundation. Matching the
Python package requires a complete document model, geometry operations,
general serialization and versioned behavioral tests.</p>
<div class="summary"><strong>Status: plan, not completed parity.</strong><br>
Target PyPI distribution 8.32.1; its audited wheel reports runtime 8.32.2.
Keep Python 8.17.0 as the Clay regression oracle. No production decoder
changes were made for this audit.</div>
<div class="facts">
<div class="fact"><strong>212</strong><span>Exported Python classes, including enums<br>Not a parity percentage</span></div>
<div class="fact"><strong>13</strong><span>Ordered work packages, P00–P12<br>Each has an acceptance gate</span></div>
<div class="fact"><strong>11</strong><span>Existing Rust workspace tests passed<br>Not a conformance suite</span></div>
</div>
</header>
{sections}
<footer>Generated from the adjacent Markdown sources with
<code>python tools/parity/render_handoff.py</code>. No external fonts, scripts
or image dependencies. Verify freshness with <code>--check</code>.</footer>
</main>
</div>
</body>
</html>
'''


class LinkCheck(HTMLParser):
    def __init__(self):
        super().__init__()
        self.ids = set()
        self.links = []

    def handle_starttag(self, tag, attrs):
        attributes = dict(attrs)
        if "id" in attributes:
            identifier = attributes["id"]
            if identifier in self.ids:
                raise ValueError(f"duplicate HTML id: {identifier}")
            self.ids.add(identifier)
        if tag == "a" and "href" in attributes:
            self.links.append(attributes["href"])

    def validate(self):
        for link in self.links:
            parts = urlsplit(link)
            if parts.scheme or parts.netloc:
                continue
            if parts.path and not (DOCS / unquote(parts.path)).is_file():
                raise ValueError(f"missing local link: {link}")
            if not parts.path and parts.fragment not in self.ids:
                raise ValueError(f"missing HTML anchor: {link}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    rendered = render()
    check = LinkCheck()
    check.feed(rendered)
    check.close()
    check.validate()
    if args.check:
        if not OUTPUT.is_file() or OUTPUT.read_text(encoding="utf-8") != rendered:
            parser.error("docs/parity/index.html is missing or stale; regenerate it")
    else:
        OUTPUT.write_text(rendered, encoding="utf-8")
    print(f"{'Checked' if args.check else 'Rendered'} {OUTPUT}: "
          f"{len(rendered.encode('utf-8'))} bytes, {len(check.ids)} IDs, "
          f"{len(check.links)} links scanned (local targets/anchors validated)")


if __name__ == "__main__":
    main()
