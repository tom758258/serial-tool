"""Generate and verify the tracked Serial Tool Desktop and CLI offline Help bundles."""

import argparse
import difflib
import html
import itertools
import re
from html.parser import HTMLParser
from pathlib import Path
from tempfile import TemporaryDirectory
from urllib.parse import unquote, urlsplit

import markdown
from markdown.extensions.toc import TocExtension, slugify_unicode


ROOT = Path(__file__).resolve().parents[1]
OUTPUT = ROOT / "apps/desktop/public/help"
CLI_OUTPUT = ROOT / "crates/serial-tool-cli/help"
DESKTOP_NAMES = {"en": "desktop.html", "zh-TW": "desktop.zh-TW.html"}
CLI_NAMES = {"en": "cli.html", "zh-TW": "cli.zh-TW.html"}
BUNDLES = (
    (ROOT / "docs/desktop", OUTPUT, DESKTOP_NAMES),
    (ROOT / "docs/cli", CLI_OUTPUT, CLI_NAMES),
)
TOKENS = ("{{lang}}", "{{title}}", "{{content}}", "{{en_href}}", "{{zh_href}}")
HTML_WARNING = "<!-- Generated file. Do not edit directly. -->"


class HelpLinks(HTMLParser):
    def __init__(self, names):
        super().__init__()
        self.languages = {}
        self.names = set(names.values())

    def handle_starttag(self, tag, attrs):
        attrs = dict(attrs)
        if tag != "a" or "href" not in attrs:
            return
        href = attrs["href"]
        url = urlsplit(href)
        public = url.scheme == "https" and bool(url.netloc)
        bundled = (
            not url.scheme
            and not url.netloc
            and not url.query
            and not url.fragment
            and url.path in self.names
        )
        if not (public or bundled):
            raise ValueError(f"Unpackaged or unsafe Help link: {href}")
        if not public and unquote(url.path).lower().endswith(".md"):
            raise ValueError(f"Repository-local Markdown link is not allowed: {href}")
        if "hreflang" in attrs:
            self.languages[attrs["hreflang"]] = href


def render(source, lang, template, names=DESKTOP_NAMES):
    if any(template.count(token) != 1 for token in TOKENS):
        raise ValueError("Template must contain all Help tokens exactly once")
    title = re.search(r"^# (.+)$", source, re.MULTILINE)
    if title is None:
        raise ValueError("Help source needs an H1 title")
    content = markdown.markdown(source, extensions=[
        "fenced_code", "tables", "sane_lists",
        TocExtension(slugify=slugify_unicode),
    ])
    values = {
        "{{lang}}": html.escape(lang),
        "{{title}}": html.escape(title[1]),
        "{{content}}": content,
        "{{en_href}}": names["en"],
        "{{zh_href}}": names["zh-TW"],
    }
    result = re.sub(r"\{\{\w+\}\}", lambda match: values.get(match[0], match[0]), template)
    result = result.replace("<!DOCTYPE html>\n", f"<!DOCTYPE html>\n{HTML_WARNING}\n", 1)
    if re.search(r"\{\{\w+\}\}", result):
        raise ValueError("Unexpanded template placeholder in Help output")
    links = HelpLinks(names)
    links.feed(result)
    if links.languages != names:
        raise ValueError(f"Help language links must point to {names}")
    return result


def generate_bundle(sources, output, names):
    template = (ROOT / "docs/help/template.html").read_text(encoding="utf-8")
    output.mkdir(parents=True, exist_ok=True)
    for source_name, lang in (("USER_GUIDE.md", "en"), ("USER_GUIDE.zh-TW.md", "zh-TW")):
        text = (sources / source_name).read_text(encoding="utf-8")
        (output / names[lang]).write_text(
            render(text, lang, template, names), encoding="utf-8", newline="\n"
        )
    css = (ROOT / "docs/help/help.css").read_text(encoding="utf-8")
    (output / "help.css").write_text(
        "/* Generated; do not edit directly. */\n" + css,
        encoding="utf-8", newline="\n",
    )


def generate(output=None):
    # --output-dir preserves the old Desktop-only override used by maintainers.
    if output is not None:
        generate_bundle(ROOT / "docs/desktop", output, DESKTOP_NAMES)
    else:
        for sources, directory, names in BUNDLES:
            generate_bundle(sources, directory, names)


def check():
    with TemporaryDirectory(prefix="serial-help-") as temporary:
        for index, (sources, output, names) in enumerate(BUNDLES):
            generated = Path(temporary) / str(index)
            generate_bundle(sources, generated, names)
            for name in (*names.values(), "help.css"):
                tracked = output / name
                actual = tracked.read_text(encoding="utf-8") if tracked.is_file() else ""
                expected = (generated / name).read_text(encoding="utf-8")
                if actual != expected:
                    diff = difflib.unified_diff(
                        actual.splitlines(keepends=True),
                        expected.splitlines(keepends=True),
                        fromfile=f"tracked/{name}",
                        tofile=f"generated/{name}",
                    )
                    print("".join(itertools.islice(diff, 60)))
                    raise ValueError(
                        f"Help bundle is out of sync: {name}; run python scripts/generate_help.py"
                    )
    print("Desktop and CLI Help bundles are synchronized and verified.")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true", help="Verify tracked output without changing it")
    parser.add_argument("--output-dir", type=Path, default=None)
    args = parser.parse_args()
    try:
        if args.check:
            check()
        else:
            generate(args.output_dir)
    except ValueError as error:
        parser.exit(1, f"Error: {error}\n")


if __name__ == "__main__":
    main()
