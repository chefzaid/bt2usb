"""Script linter regression tests: maskfile recipe extraction and line mapping."""

import contextlib
import io
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import lint_scripts

MASKFILE = """\
# Tasks

## build

> Build it

**OPTIONS**
* release
    * flags: --release
    * desc: Optimized build
* dry-run
    * flags: --dry-run
    * desc: Print instead of building

**Notes:**
* not-an-option

```bash
if [[ "${release}" == "true" && "${dry_run}" != "true" ]]; then echo release; fi
```

A second block documents the recipe and is not its script:

```bash
echo ignored
```

## greet (name) [greeting]

```sh
echo "${greeting:-hello} ${name}"
```

## report

```python
print("not a shell recipe")
```
"""


class RecipeTests(unittest.TestCase):
    def setUp(self):
        self.recipes = {recipe.name: recipe for recipe in lint_scripts.recipes(MASKFILE)}

    def test_first_block_under_each_heading_is_the_script(self):
        self.assertEqual(list(self.recipes), ["build", "greet", "report"])
        self.assertEqual(self.recipes["build"].language, "bash")
        self.assertEqual(len(self.recipes["build"].lines), 1)
        self.assertIn("release", self.recipes["build"].lines[0])

    def test_options_and_arguments_become_variables(self):
        self.assertEqual(self.recipes["build"].variables, ["release", "dry_run"])
        self.assertEqual(self.recipes["greet"].variables, ["name", "greeting"])
        self.assertEqual(self.recipes["report"].variables, [])

    def test_padded_script_keeps_maskfile_line_numbers(self):
        lines = MASKFILE.splitlines()
        for recipe in self.recipes.values():
            padded = lint_scripts.padded(recipe).splitlines()
            for offset, line in enumerate(recipe.lines):
                number = recipe.first_line + offset
                self.assertEqual(padded[number - 1], lines[number - 1])
        build = lint_scripts.padded(self.recipes["build"]).splitlines()[0]
        self.assertEqual(build, ': "${release=}" "${dry_run=}"')
        report = lint_scripts.padded(self.recipes["report"]).splitlines()[0]
        self.assertEqual(report, "")

    def test_only_shell_recipes_are_checked(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "maskfile.md").write_text(MASKFILE, encoding="utf-8")
            checked = [recipe.name for recipe in lint_scripts.shell_recipes(root)]
        self.assertEqual(checked, ["build", "greet"])


@unittest.skipUnless(shutil.which("shellcheck"), "ShellCheck is not installed")
class ShellCheckTests(unittest.TestCase):
    def findings(self, text):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            (root / "maskfile.md").write_text(text, encoding="utf-8")
            checked = lint_scripts.shell_recipes(root)
            return lint_scripts.check_recipes(checked, root, shutil.which("shellcheck"))

    def test_clean_recipes_pass(self):
        self.assertEqual(self.findings(MASKFILE), [])

    def test_finding_names_the_maskfile_line_and_recipe(self):
        text = MASKFILE.replace('echo "${greeting:-hello} ${name}"', 'words="a b"\necho $words')
        line = text.splitlines().index("echo $words") + 1
        found = self.findings(text)
        self.assertEqual(len(found), 1, found)
        self.assertTrue(found[0].startswith(f"maskfile.md:{line}:6: "), found)
        self.assertIn("[SC2086] (recipe greet)", found[0])


class MainTests(unittest.TestCase):
    def test_missing_tool_exits_2(self):
        stderr = io.StringIO()
        with (
            patch.object(lint_scripts.shutil, "which", return_value=None),
            contextlib.redirect_stderr(stderr),
        ):
            self.assertEqual(lint_scripts.main([]), 2)
        self.assertIn("Install ruff and shellcheck first", stderr.getvalue())


if __name__ == "__main__":
    unittest.main()
