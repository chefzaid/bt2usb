"""Build the release firmware images reproducibly and hold them to their size
budgets.

Python 3.11+; standard library only. rustc writes the absolute path of each
dependency's source into the image, in the panic locations the flashed code
carries and in the debug information, so two builds of one commit under
different checkouts or Cargo homes would differ. `release.py build` remaps both
prefixes to fixed names; `release.py stage` refuses an image that still names
either one, or that is over a budget below.
"""

from __future__ import annotations

import os
import struct
from pathlib import Path

# Cargo joins this variable, the firmware target's `rustflags`, with the
# linker flags `.cargo/config.toml` sets; `RUSTFLAGS` would replace them.
TARGET_RUSTFLAGS = "CARGO_TARGET_THUMBV7EM_NONE_EABIHF_RUSTFLAGS"
CARGO_HOME_PREFIX = "/cargo"
SOURCE_PREFIX = "."
# The nRF52840's RAM starts here; every other byte an image loads is flash.
RAM_BASE = 0x2000_0000
# Bytes each release image may take. Flash covers code, read-only data, and the
# initial values of statics: under half the 804 KiB application region, which
# keeps room for a second image slot should updates over USB or BLE be added.
# RAM covers the statics, and leaves at least 168 KiB of the 232 KiB
# application RAM to the stack and its guard.
BUDGETS = {"flash": 384 * 1024, "ram": 64 * 1024}


def cargo_home() -> Path:
    """The Cargo home whose registry and git checkouts the build reads, as Cargo
    names it to rustc: made absolute, but with symbolic links kept."""
    return Path(os.environ.get("CARGO_HOME") or Path.home() / ".cargo").absolute()


def remap_flags(root: Path, home: Path) -> str:
    """The rustc flags that replace the checkout and Cargo home prefixes."""
    pairs = ((home, CARGO_HOME_PREFIX), (root.resolve(), SOURCE_PREFIX))
    if any(char.isspace() for path, _ in pairs for char in str(path)):
        # Cargo splits the variable at whitespace.
        raise ValueError("build from a checkout and Cargo home without whitespace in their paths")
    return " ".join(f"--remap-path-prefix={path}={prefix}" for path, prefix in pairs)


def unmapped_paths(image: bytes, root: Path, home: Path) -> list[str]:
    """The build-machine paths an image still names, if any."""
    paths = dict.fromkeys(str(path) for path in (root.resolve(), home, home.resolve()))
    return [path for path in paths if path.encode() in image]


def usage(image: bytes) -> dict[str, int]:
    """The flash and RAM bytes the loadable segments of an ELF32 image take."""
    if image[:6] != b"\x7fELF\x01\x01":
        raise ValueError("not a little-endian 32-bit ELF image")
    (table,) = struct.unpack_from("<I", image, 0x1C)
    entry_size, entries = struct.unpack_from("<HH", image, 0x2A)
    taken = {"flash": 0, "ram": 0}
    for index in range(entries):
        kind, _, address, _, file_size, memory_size = struct.unpack_from(
            "<6I", image, table + index * entry_size
        )
        if kind == 1:  # PT_LOAD
            taken["flash"] += file_size
            if address >= RAM_BASE:
                taken["ram"] += memory_size
    return taken


def over_budget(taken: dict[str, int]) -> list[str]:
    """Each budget the image exceeds, as `kind N > budget` text."""
    return [
        f"{kind} {taken[kind]} > {limit} bytes"
        for kind, limit in BUDGETS.items()
        if taken[kind] > limit
    ]
