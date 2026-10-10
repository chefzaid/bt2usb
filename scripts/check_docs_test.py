"""Documentation checker regression tests on a small synthetic repository."""

import contextlib
import io
import tempfile
import textwrap
import unittest
from pathlib import Path

import check_docs

CONFIG = """\
//! Constants.
pub const BLE_SCAN_DURATION_SECS: u64 = 8;
pub const BLE_CONN_INTERVAL_MIN: u16 = 6;
pub const BLE_CONN_INTERVAL_MAX: u16 = 12;
pub const BLE_SUP_TIMEOUT: u16 = 400;
pub const BLE_CONNECT_TIMEOUT_SECS: u16 = 6;
pub const BLE_RECONNECT_BACKOFF_MS: u64 = 500;
pub const BLE_FAILED_RECONNECT_HOLDOFF_MS: u64 =
    BLE_CONNECT_TIMEOUT_SECS as u64 * 1000 + BLE_RECONNECT_BACKOFF_MS;
pub const USB_PRODUCT: &str = "BT-to-USB HID Bridge";
pub const SCREEN_AUTO_OFF_ENABLED: bool = true;
pub const FLASH_PAGE_SIZE: u32 = 4096;
pub const STORAGE_FLASH_PAGE_START: u32 = 240;
pub const STORAGE_FLASH_PAGE_COUNT: u32 = 4;
pub const STORAGE_FLASH_START: u32 = STORAGE_FLASH_PAGE_START * FLASH_PAGE_SIZE;
pub const STORAGE_FLASH_END: u32 =
    STORAGE_FLASH_START + STORAGE_FLASH_PAGE_COUNT * FLASH_PAGE_SIZE;
"""

HARDWARE = """\
# Hardware

## Configuration Defaults

| Setting | Default | Meaning |
| --- | --- | --- |
| `BLE_SCAN_DURATION_SECS` | 8 | Scan window |
| `BLE_CONN_INTERVAL_MIN` / `MAX` | 6 / 12 | 7.5–15 ms |
| `BLE_SUP_TIMEOUT` | 400 | 4 s |
| `BLE_CONNECT_TIMEOUT_SECS` | 6 | Attempt timeout |
| `BLE_RECONNECT_BACKOFF_MS` | 500 | Pause |
| `BLE_FAILED_RECONNECT_HOLDOFF_MS` | 6500 | Derived |
| `USB_PRODUCT` | `BT-to-USB HID Bridge` | String descriptor |
| `SCREEN_AUTO_OFF_ENABLED` | `true` | Power-off |
| `FLASH_PAGE_SIZE` | 4096 | Page |
| `STORAGE_FLASH_PAGE_START` / `COUNT` | 240 / 4 | Pages |
| `STORAGE_FLASH_START` / `END` | `0x000F0000` / `0x000F4000` | Range |

## Memory Layout

| Region | Address range, end exclusive | Size |
| --- | --- | --- |
| SoftDevice flash | `0x00000000–0x00027000` | 156 KiB |
| Application flash | `0x00027000–0x000F0000` | 804 KiB |
| Pairing/bond storage | `0x000F0000–0x000F4000` | 16 KiB |
| Unused tail flash | `0x000F4000–0x00100000` | 48 KiB |
| SoftDevice RAM reservation | `0x20000000–0x20006000` | 24 KiB |
| Application RAM | `0x20006000–0x20040000` | 232 KiB |

The simulation build owns the whole device:

| Region | Address range, end exclusive | Size |
| --- | --- | --- |
| Flash ([memory_sim.x](../memory_sim.x)) | `0x00000000–0x00100000` | 1024 KiB |
| RAM | `0x20000000–0x20040000` | 256 KiB |
"""

GUIDE = """\
# Guide

See [hardware](hardware.md#configuration-defaults) and the
[record](#validation-record--2026-10-10). Scans last
`BLE_SCAN_DURATION_SECS` (8 s); links drop after 4 seconds (`BLE_SUP_TIMEOUT`).
Run `mask ci` or `cargo build --features embedded --bin bt2usb`, then read
`src/config.rs` and `config.rs`. Pairing uses flash pages 240–243
(`0xF0000–0xF4000`).

## Validation Record — 2026-10-10

Back then scans lasted `BLE_SCAN_DURATION_SECS` (5 s).

```sh
# A comment in a fenced block is not a heading
mask gone
```

Still the record: pages 1–2 of the pairing log, `src/old.rs`.

## After The Record

Text.
"""


class CheckDocsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.write("src/config.rs", CONFIG)
        self.write("docs/hardware.md", HARDWARE)
        self.write("docs/guide.md", GUIDE)
        self.write(
            "memory_sd.x",
            textwrap.dedent("""\
            MEMORY
            {
                /* FLASH : ORIGIN = 0x0, LENGTH = 1K in a comment is ignored */
                FLASH : ORIGIN = 0x00027000, LENGTH = 804K
                RAM : ORIGIN = 0x20006000, LENGTH = 232K
            }
            """),
        )
        self.write(
            "memory_sim.x",
            textwrap.dedent("""\
            MEMORY
            {
                FLASH : ORIGIN = 0x00000000, LENGTH = 1024K
                RAM : ORIGIN = 0x20000000, LENGTH = 256K
            }
            """),
        )
        self.write("maskfile.md", "# Tasks\n\n## ci\n\n## build-release\n")
        self.write(
            "Cargo.toml",
            textwrap.dedent("""\
            [package]
            name = "bt2usb"
            version = "0.1.0"

            [[bin]]
            name = "bt2usb"
            path = "src/main.rs"

            [features]
            default = []
            embedded = []
            sim = []
            """),
        )

    def write(self, name, text):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(text, encoding="utf-8")

    def edit(self, name, old, new):
        path = self.root / name
        text = path.read_text(encoding="utf-8")
        self.assertIn(old, text)
        path.write_text(text.replace(old, new, 1), encoding="utf-8")

    def append(self, name, text):
        path = self.root / name
        path.write_text(path.read_text(encoding="utf-8") + text, encoding="utf-8")

    def findings(self, *checks):
        return check_docs.run(self.root, checks or check_docs.CHECKS)

    def messages(self, *checks):
        return [f"{f.path}:{f.line}: {f.message}" for f in self.findings(*checks)]

    def test_consistent_repository_passes(self):
        self.assertEqual(self.findings(), [])

    def test_missing_link_target(self):
        self.append("docs/guide.md", "\nSee [gone](missing.md).\n")
        self.assertEqual(
            self.messages("links"), ["docs/guide.md:25: link target missing.md does not exist"]
        )

    def test_link_outside_the_repository(self):
        self.append("docs/guide.md", "\nSee [up](../../elsewhere.md).\n")
        self.assertEqual(len(self.findings("links")), 1)

    def test_missing_anchor(self):
        self.append("docs/guide.md", "\nSee [defaults](hardware.md#defaults).\n")
        self.assertEqual(
            self.messages("links"),
            ["docs/guide.md:25: docs/hardware.md has no heading for #defaults"],
        )

    def test_anchor_rules(self):
        self.append(
            "docs/guide.md",
            textwrap.dedent("""
            ## Text
            ## Text
            ## The `config.rs` File
            <a id="custom"></a>
            [a](#text) [b](#text-1) [c](#the-configrs-file) [d](#custom) [e](#text-2)
            """),
        )
        # Two "Text" headings are #text and #text-1; there is no third.
        self.assertEqual(
            self.messages("links"), ["docs/guide.md:29: docs/guide.md has no heading for #text-2"]
        )

    def test_links_in_code_are_ignored(self):
        self.append("docs/guide.md", "\n`[x](missing.md)`\n\n```\n[y](gone.md)\n```\n")
        self.assertEqual(self.findings("links"), [])

    def test_external_links_are_ignored(self):
        self.append("docs/guide.md", "\n[site](https://example.com/x#y) [m](mailto:a@b.c)\n")
        self.assertEqual(self.findings("links"), [])

    def test_table_value_disagrees(self):
        self.edit(
            "docs/hardware.md",
            "| `BLE_SCAN_DURATION_SECS` | 8 |",
            "| `BLE_SCAN_DURATION_SECS` | 9 |",
        )
        self.assertEqual(
            self.messages("config"),
            [
                (
                    "docs/hardware.md:7: BLE_SCAN_DURATION_SECS is documented as 9 "
                    "but src/config.rs:2 sets 8"
                )
            ],
        )

    def test_shorthand_names_are_expanded(self):
        self.edit("docs/hardware.md", "| 6 / 12 |", "| 6 / 13 |")
        self.assertEqual(
            self.messages("config"),
            [
                (
                    "docs/hardware.md:8: BLE_CONN_INTERVAL_MAX is documented as 13 "
                    "but src/config.rs:4 sets 12"
                )
            ],
        )

    def test_derived_values_strings_and_booleans(self):
        self.edit("docs/hardware.md", "| 6500 |", "| 6000 |")
        self.edit("docs/hardware.md", "| `BT-to-USB HID Bridge` |", "| `bt2usb` |")
        self.edit("docs/hardware.md", "| `true` |", "| `false` |")
        self.edit("docs/hardware.md", "`0x000F4000` |", "`0x000F5000` |")
        found = self.messages("config")
        self.assertEqual(len(found), 4, found)
        self.assertIn("BLE_FAILED_RECONNECT_HOLDOFF_MS is documented as 6000", found[0])
        self.assertIn('sets "BT-to-USB HID Bridge"', found[1])
        self.assertIn("sets true", found[2])
        self.assertIn("STORAGE_FLASH_END is documented as `0x000F5000`", found[3])

    def test_constant_missing_from_table(self):
        self.append("src/config.rs", "pub const NEW_LIMIT: usize = 3;\n")
        self.assertEqual(
            self.messages("config"),
            [
                (
                    "docs/hardware.md:3: src/config.rs:18 defines NEW_LIMIT = 3, but the "
                    "Configuration Defaults table does not list it"
                )
            ],
        )

    def test_table_names_unknown_constant(self):
        self.edit("docs/hardware.md", "`BLE_SCAN_DURATION_SECS`", "`BLE_SCAN_SECS`")
        found = self.messages("config")
        self.assertIn("docs/hardware.md:7: BLE_SCAN_SECS is not a constant in src/config.rs", found)
        self.assertEqual(len(found), 2, found)  # and BLE_SCAN_DURATION_SECS is unlisted

    def test_row_with_missing_value(self):
        self.edit("docs/hardware.md", "| 240 / 4 |", "| 240 |")
        self.assertEqual(self.messages("config"), ["docs/hardware.md:16: 2 names but 1 values"])

    def test_inline_mentions_with_units(self):
        self.edit(
            "src/config.rs", "BLE_SCAN_DURATION_SECS: u64 = 8", "BLE_SCAN_DURATION_SECS: u64 = 9"
        )
        self.edit("src/config.rs", "BLE_SUP_TIMEOUT: u16 = 400", "BLE_SUP_TIMEOUT: u16 = 300")
        found = [m for m in self.messages("config") if m.startswith("docs/guide.md")]
        self.assertEqual(
            found,
            [
                (
                    "docs/guide.md:5: '4 seconds (`BLE_SUP_TIMEOUT`)' disagrees with "
                    "src/config.rs:5, which sets BLE_SUP_TIMEOUT = 300"
                ),
                (
                    "docs/guide.md:5: '`BLE_SCAN_DURATION_SECS` (8 s)' disagrees with "
                    "src/config.rs:2, which sets BLE_SCAN_DURATION_SECS = 9"
                ),
            ],
        )

    def test_inline_mentions_that_cannot_be_compared_pass(self):
        self.append(
            "docs/guide.md", "\n`USB_PRODUCT` (2) and `BLE_SUP_TIMEOUT` (60 fast windows).\n"
        )
        self.assertEqual(self.findings("config"), [])

    def test_ignore_marker(self):
        self.append(
            "docs/guide.md",
            "\nIt was `BLE_SCAN_DURATION_SECS` (5 s). <!-- check-docs: ignore -->\n",
        )
        self.assertEqual(self.findings(), [])

    def test_record_ends_at_next_heading_of_same_level(self):
        self.append("docs/guide.md", "Scans last `BLE_SCAN_DURATION_SECS` (5 s).\n")
        self.assertEqual(
            self.messages("config"),
            [
                (
                    "docs/guide.md:24: '`BLE_SCAN_DURATION_SECS` (5 s)' disagrees with "
                    "src/config.rs:2, which sets BLE_SCAN_DURATION_SECS = 8"
                )
            ],
        )

    def test_memory_table_disagrees(self):
        self.edit("memory_sd.x", "LENGTH = 804K", "LENGTH = 800K")
        found = self.messages("memory")
        self.assertIn(
            "docs/hardware.md:24: Application flash is documented as 0x00027000–0x000f0000 "
            "but the linker scripts and src/config.rs give 0x00027000–0x000ef000",
            found,
        )
        self.assertIn(
            "docs/hardware.md:24: Application flash is documented as 804 KiB but spans 800 KiB",
            found,
        )

    def test_memory_size_disagrees(self):
        self.edit("docs/hardware.md", "| 232 KiB |", "| 230 KiB |")
        self.assertEqual(
            self.messages("memory"),
            ["docs/hardware.md:28: Application RAM is documented as 230 KiB but spans 232 KiB"],
        )

    def test_simulation_rows(self):
        self.edit("memory_sim.x", "LENGTH = 256K", "LENGTH = 128K")
        found = self.messages("memory")
        self.assertEqual(len(found), 2, found)
        self.assertTrue(all("RAM" in message for message in found))

    def test_prose_range_with_wrong_end(self):
        self.append("docs/guide.md", "\nThe store is `0xF0000–0xF5000`.\n")
        self.assertEqual(
            self.messages("memory"),
            [
                (
                    "docs/guide.md:25: `0xF0000–0xF5000` starts Pairing/bond storage but ends at "
                    "0xf5000; the region ends at 0xf4000"
                )
            ],
        )

    def test_page_range(self):
        self.append(
            "docs/guide.md",
            "\nThe pairing region is pages 240–244.\nRead pages 3-9 of the manual.\n",
        )
        self.assertEqual(
            self.messages("memory"),
            [
                (
                    "docs/guide.md:25: pages 240–244 disagree with STORAGE_FLASH_PAGE_START/COUNT, "
                    "which reserve pages 240–243 (0xf0000–0xf4000)"
                )
            ],
        )

    def test_unknown_mask_recipe_binary_and_feature(self):
        self.append(
            "docs/guide.md",
            textwrap.dedent("""
            Run `mask deploy`, then:

            ```sh
            cargo run --bin bt2usb-tool --features embedded,turbo
            ```
            """),
        )
        self.assertEqual(
            self.messages("commands"),
            [
                "docs/guide.md:25: mask has no recipe named 'deploy'",
                "docs/guide.md:28: Cargo.toml has no binary named 'bt2usb-tool'",
                "docs/guide.md:28: Cargo.toml has no feature named 'turbo'",
            ],
        )

    def test_missing_paths(self):
        self.append(
            "docs/guide.md",
            "\nSee `src/ble/gone.rs`, `gone.rs`, `ble/config.rs`, and `memory.x`.\n",
        )
        self.assertEqual(
            self.messages("commands"),
            [
                "docs/guide.md:25: no file in the repository matches ble/config.rs",
                "docs/guide.md:25: no file in the repository matches gone.rs",
                "docs/guide.md:25: no file in the repository matches src/ble/gone.rs",
            ],
        )

    def test_paths_that_may_be_absent(self):
        self.append(
            "docs/guide.md",
            "\n`OUT_DIR/memory.x`, `target/x/doc/index.html`, "
            "`src/hid/held.rs`, `*_tests.rs`, `<file>.rs`, `.rs`.\n",
        )
        self.assertEqual(self.findings("commands"), [])

    def test_proposed_adr_may_name_new_files(self):
        self.write(
            "docs/adr/0099-new.md",
            "# ADR 0099: New\n\n- Status: Proposed\n\nAdd `src/ble/new.rs`.\n",
        )
        self.assertEqual(self.findings("commands"), [])
        self.edit("docs/adr/0099-new.md", "Proposed", "Accepted")
        self.assertEqual(len(self.findings("commands")), 1)

    def test_main_reports_and_exits(self):
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(check_docs.main(["--root", str(self.root)]), 0)
        self.assertIn("Documentation checks passed", output.getvalue())
        self.append("docs/guide.md", "\n[x](gone.md)\n")
        output = io.StringIO()
        with contextlib.redirect_stdout(output), contextlib.redirect_stderr(io.StringIO()):
            self.assertEqual(check_docs.main(["--root", str(self.root), "--only", "links"]), 1)
        self.assertIn(
            "docs/guide.md:25: [links] link target gone.md does not exist", output.getvalue()
        )


if __name__ == "__main__":
    unittest.main()
