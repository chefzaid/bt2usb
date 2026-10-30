"""Tests for the reproducible release build and the image size budgets; no
firmware build."""

import struct
import unittest
from pathlib import Path
from unittest.mock import patch

import release
import release_image

GNU_STACK = 0x6474E551
# The program headers of the release bridge at commit 3a439a5: vector table,
# code, read-only data, `.data` (loaded from flash), `.bss` and `.uninit`.
BRIDGE = (
    (1, 0x00027000, 0x100, 0x100),
    (1, 0x00027100, 0x1ED48, 0x1ED48),
    (1, 0x00045E48, 0x30FC, 0x30FC),
    (1, 0x20006000, 0x668, 0x668),
    (1, 0x20006668, 0, 0x6764),
    (GNU_STACK, 0, 0, 0),
)


def elf_image(segments=BRIDGE, payload=b""):
    """A little-endian ELF32 file with these `(type, address, file size, memory
    size)` program headers, followed by `payload`."""
    header = bytearray(52)
    header[:6] = b"\x7fELF\x01\x01"
    struct.pack_into("<I", header, 0x1C, len(header))
    struct.pack_into("<HH", header, 0x2A, 32, len(segments))
    table = b"".join(
        struct.pack("<8I", kind, 0, address, address, file_size, memory_size, 0, 4)
        for kind, address, file_size, memory_size in segments
    )
    return bytes(header) + table + payload


class ReleaseImageTests(unittest.TestCase):
    def test_usage_counts_loaded_bytes_as_flash_and_ram_segments_as_ram(self):
        # `llvm-size`: text 139,076 and data 1,640 in flash; data and bss
        # 1,640 + 26,468 in RAM.
        self.assertEqual(release_image.usage(elf_image()), {"flash": 140_716, "ram": 28_108})
        for image in (b"", b"\x7fELF\x02\x01" + bytes(64), b"MZ" + bytes(64)):
            with self.subTest(image=image[:6]), self.assertRaises(ValueError):
                release_image.usage(image)

    def test_budgets_name_each_exceeded_kind(self):
        flash, ram = release_image.BUDGETS["flash"], release_image.BUDGETS["ram"]
        self.assertEqual(release_image.over_budget({"flash": flash, "ram": ram}), [])
        self.assertEqual(
            release_image.over_budget({"flash": flash + 1, "ram": ram + 1}),
            [f"flash {flash + 1} > {flash} bytes", f"ram {ram + 1} > {ram} bytes"],
        )

    def test_remap_covers_the_checkout_and_cargo_home_and_refuses_spaces(self):
        root, home = Path("/work/bt2usb").resolve(), Path("/home/me/.cargo").resolve()
        self.assertEqual(
            release_image.remap_flags(root, home),
            f"--remap-path-prefix={home}=/cargo --remap-path-prefix={root}=.",
        )
        with self.assertRaises(ValueError):
            release_image.remap_flags(root / "my work", home)
        image = f"panicked at {home}/registry/src/x.rs and ./src/main.rs".encode()
        self.assertEqual(release_image.unmapped_paths(image, root, home), [str(home)])
        self.assertEqual(release_image.unmapped_paths(b"/cargo/registry", root, home), [])

    def test_build_remaps_paths_and_defaults_to_info_logging(self):
        root, home = Path("/work/bt2usb"), Path("/opt/cargo")
        with (
            patch.dict(release.os.environ, {"CARGO_HOME": str(home)}, clear=True),
            patch.object(release.subprocess, "run") as run,
        ):
            release.build_release(root)
        args, env = run.call_args.args[0], run.call_args.kwargs["env"]
        self.assertEqual(args[:3], ["cargo", "build", "--locked"])
        self.assertEqual(args[args.index("--target") + 1], release.TARGET)
        self.assertIn("--release", args)
        self.assertEqual(env["DEFMT_LOG"], "info")
        self.assertEqual(
            env[release_image.TARGET_RUSTFLAGS],
            release_image.remap_flags(root, home.absolute()),
        )


if __name__ == "__main__":
    unittest.main()
