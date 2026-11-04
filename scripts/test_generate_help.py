"""Focused checks for Desktop Help rendering and link safety."""
import unittest
from generate_help import ROOT, render


class HelpGeneratorTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.template = (ROOT / "docs/help/template.html").read_text(encoding="utf-8")

    def test_deterministic_sections_tables_and_code(self):
        source = "# User Guide\n\n## 章節\n\n## 章節\n\n- **bold**\n\n| A | B |\n| --- | --- |\n| 1 | 2 |\n\n```text\nhello\n```\n"
        output = render(source, "zh-TW", self.template)
        self.assertEqual(output, render(source, "zh-TW", self.template))
        for fragment in ('id="section-1"', 'id="section-2"', "<table>",
                         "<strong>bold</strong>", '<pre><code class="language-text">'):
            self.assertIn(fragment, output)

    def test_validated_language_links_and_local_markdown(self):
        result = render("# Guide\n\n[Official](https://example.com/guide)", "en", self.template)
        self.assertIn('href="https://example.com/guide"', result)
        for bad in ("README.md", "../missing.md", "javascript:alert(1)", "file:///tmp/docs.md"):
            with self.subTest(bad=bad), self.assertRaises(ValueError):
                render("# Guide\n\n[bad](" + bad + ")", "en", self.template)

    def test_invalid_template_fails(self):
        for tpl in (self.template.replace("{{content}}", ""), self.template + "{{unknown}}",
                    self.template.replace('href="desktop.zh-TW.html"', 'href="missing.html"')):
            with self.subTest(tpl=tpl), self.assertRaises(ValueError):
                render("# Guide", "en", tpl)


if __name__ == "__main__":
    unittest.main()
