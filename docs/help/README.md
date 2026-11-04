# Desktop offline Help maintenance

Canonical user-facing content is maintained only in `docs/desktop/USER_GUIDE.md`
and `docs/desktop/USER_GUIDE.zh-TW.md`. The shared presentation sources are
`docs/help/template.html` and `docs/help/help.css`. Do not manually edit generated files.

From the repository root with Python 3.12, run:

```sh
python scripts/generate_help.py
python -B scripts/test_generate_help.py
python scripts/generate_help.py --check
```

The generator uses only Python's standard library; no third-party packages need to
be installed (see `scripts/requirements-help.txt`). It supports the current
guide Markdown: headings, paragraphs, emphasis, inline code and links, bullet
lists, tables, and fenced code. New markdown features must be intentionally
supported and tested before adding them to guides.

The complete tracked bundle is `apps/desktop/public/help/`:
`desktop.html`, `desktop.zh-TW.html` and `help.css`.
The `--check` mode regenerates into a temporary directory and compares
all three files without changing working files. Unpackaged Markdown links are
rejected. Keep developer-document references in the language-appropriate
README rather than adding them to the offline operator guide. The Help
language navigation links point only to the bundled HTML pages.

Vite copies the tracked assets into its built frontend and Tauri embeds them.
The Desktop executable needs no Python, separate HTML sidecars, or internet
connection to read Help. Help inherits current language and theme upon opening;
close and reopen it to use a changed main-window preference. System theme
responds to OS color-scheme changes.
