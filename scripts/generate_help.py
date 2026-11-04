"""Generate and verify self-contained bilingual Desktop Help assets."""

import argparse
import html
import re
from html.parser import HTMLParser
from pathlib import Path
from tempfile import TemporaryDirectory
from urllib.parse import urlsplit

ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "apps/desktop/public/help"
SOURCES = (("USER_GUIDE.md", "desktop.html", "en"),
           ("USER_GUIDE.zh-TW.md", "desktop.zh-TW.html", "zh-TW"))
FILES = ("desktop.html", "desktop.zh-TW.html", "help.css")
TOKENS = ("{{lang}}", "{{title}}", "{{content}}")
BANNER = "<!-- Generated file. Do not edit directly. -->"
MD_LINK = re.compile(r"\[([^\]]+)\]\(([^)]+)\)")


def render_inline(value):
    text = html.escape(value, quote=True)

    def replace_link(match):
        label, target = match.groups()
        parsed = urlsplit(target)
        if not (parsed.scheme == "https" and parsed.netloc):
            raise ValueError("Help references an unpackaged or unsafe link: " + target)
        return '<a href="' + html.escape(target, quote=True) + '">' + label + "</a>"

    text = MD_LINK.sub(replace_link, text)
    text = re.sub(r"`([^`]+)`", lambda m: "<code>" + m[1] + "</code>", text)
    return re.sub(r"\*\*(.+?)\*\*", lambda m: "<strong>" + m[1] + "</strong>", text)


def render_markdown(source):
    """Render only the intentionally small operator-guide Markdown subset."""
    lines = source.splitlines()
    result, i, section = [], 0, 0
    while i < len(lines):
        line = lines[i].strip()
        if not line:
            i += 1
            continue
        if line.startswith(("```", "~~~")):
            marker, language = line[:3], line[3:].strip()
            code = []
            i += 1
            while i < len(lines) and not lines[i].strip().startswith(marker):
                code.append(lines[i])
                i += 1
            if i == len(lines):
                raise ValueError("Unterminated Help code fence")
            kind = ' class="language-' + html.escape(language, quote=True) + '"' if language else ""
            result.append("<pre><code" + kind + ">" + html.escape("\n".join(code)) + "</code></pre>")
            i += 1
            continue
        match = re.match(r"^(#{1,3})\s+(.+)$", line)
        if match:
            level, name = len(match[1]), match[2]
            anchor = ""
            if level > 1:
                section += 1
                anchor = f' id="section-{section}"'
            result.append(f"<h{level}{anchor}>{render_inline(name)}</h{level}>")
            i += 1
            continue
        if line.startswith("- "):
            items = []
            while i < len(lines) and lines[i].strip().startswith("- "):
                items.append("<li>" + render_inline(lines[i].strip()[2:]) + "</li>")
                i += 1
            result.append("<ul>" + "".join(items) + "</ul>")
            continue
        if line.startswith("|") and i + 1 < len(lines) and re.fullmatch(r"\|[\s:|\-]+\|", lines[i + 1].strip()):
            headers = [cell.strip() for cell in line.strip("|").split("|")]
            i += 2
            rows = []
            while i < len(lines) and lines[i].strip().startswith("|"):
                cells = [cell.strip() for cell in lines[i].strip().strip("|").split("|")]
                if len(cells) != len(headers):
                    raise ValueError("Help table row has the wrong number of cells")
                rows.append("<tr>" + "".join("<td>" + render_inline(cell) + "</td>" for cell in cells) + "</tr>")
                i += 1
            result.append("<table><thead><tr>" + "".join(
                "<th>" + render_inline(cell) + "</th>" for cell in headers
            ) + "</tr></thead><tbody>" + "".join(rows) + "</tbody></table>")
            continue
        paragraph = [line]
        i += 1
        while i < len(lines):
            following = lines[i].strip()
            if not following or re.match(r"^#{1,3}\s+", following) or following.startswith(
                ("- ", "|", "~~~", "```")
            ):
                break
            paragraph.append(following)
            i += 1
        result.append("<p>" + render_inline(" ".join(paragraph)) + "</p>")
    return "\n".join(result)


class HelpLinks(HTMLParser):
    def __init__(self):
        super().__init__()
        self.languages = {}

    def handle_starttag(self, tag, attrs):
        if tag != "a":
            return
        attrs = dict(attrs)
        target = attrs.get("href", "")
        parsed = urlsplit(target)
        if not ((parsed.scheme == "https" and parsed.netloc) or (
            parsed.scheme == "" and parsed.netloc == "" and parsed.path in
            ("desktop.html", "desktop.zh-TW.html")
        )):
            raise ValueError("Invalid Help HTML anchor: " + target)
        if "hreflang" in attrs:
            self.languages[attrs["hreflang"]] = target


def render(source, lang, template):
    if any(template.count(token) != 1 for token in TOKENS):
        raise ValueError("Help template placeholders must occur exactly once")
    title = re.search(r"^# (.+)$", source, re.MULTILINE)
    if title is None:
        raise ValueError("Help source must have a level-one title")
    values = {"{{lang}}": html.escape(lang), "{{title}}": html.escape(title[1]),
              "{{content}}": render_markdown(source)}
    generated = re.sub(r"\{\{\w+\}\}", lambda m: values.get(m[0], m[0]), template)
    generated = generated.replace("<!DOCTYPE html>\n", "<!DOCTYPE html>\n" + BANNER + "\n", 1)
    if re.search(r"\{\{\w+\}\}", generated):
        raise ValueError("Unexpanded Help placeholder")
    links = HelpLinks()
    links.feed(generated)
    if links.languages != {language: name for _, name, language in SOURCES}:
        raise ValueError("Help language links do not match bundled HTML")
    return generated


def generate(destination):
    template = (ROOT / "docs/help/template.html").read_text(encoding="utf-8")
    destination.mkdir(parents=True, exist_ok=True)
    for name, output, lang in SOURCES:
        source = (ROOT / "docs/desktop" / name).read_text(encoding="utf-8")
        (destination / output).write_text(render(source, lang, template), encoding="utf-8", newline="\n")
    css = (ROOT / "docs/help/help.css").read_text(encoding="utf-8")
    (destination / "help.css").write_text("/* Generated; do not edit directly. */\n" + css,
                                          encoding="utf-8", newline="\n")


def check():
    with TemporaryDirectory(prefix="serial-help-") as temp:
        candidate = Path(temp)
        generate(candidate)
        for filename in FILES:
            tracked = OUTPUT / filename
            if not tracked.is_file() or tracked.read_text(encoding="utf-8") != (
                candidate / filename
            ).read_text(encoding="utf-8"):
                raise ValueError("Tracked Help bundle out of sync: " + filename)
    print("Desktop Help files synchronized")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    parser.add_argument("--output-dir", type=Path, default=OUTPUT)
    args = parser.parse_args()
    try:
        if args.check:
            check()
        else:
            generate(args.output_dir)
    except ValueError as exc:
        parser.exit(1, "Error: " + str(exc) + "\n")


if __name__ == "__main__":
    main()
