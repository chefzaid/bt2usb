#!/usr/bin/env python3
"""Validate release identity, package the exact successful CI build, and fill
the release notes template for its draft.

Python 3.11+; standard library only. This helper never publishes a release.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import shutil
import string
import subprocess
import tomllib
from pathlib import Path

TARGET = "thumbv7em-none-eabihf"
INPUTS = ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
FIRMWARE = ("bt2usb.elf", "bt2usb-selftest.elf", "bt2usb.hex")
STAGED = (*FIRMWARE, *INPUTS, "BUILD-INFO.json")
NOTES_TEMPLATE = Path(".github", "release-notes.md")
SEMVER = re.compile(
    r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)"
    r"(?:-((?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*)"
    r"(?:\.(?:0|[1-9][0-9]*|[0-9]*[A-Za-z-][0-9A-Za-z-]*))*))?"
    r"(?:\+([0-9A-Za-z-]+(?:\.[0-9A-Za-z-]+)*))?"
)


class ReleaseError(ValueError):
    """A release consistency or integrity check failed."""


def package_version(manifest: Path) -> str:
    with manifest.open("rb") as handle:
        package = tomllib.load(handle).get("package", {})
    version = package.get("version")
    if package.get("name") != "bt2usb" or not isinstance(version, str):
        raise ReleaseError("manifest must contain bt2usb's explicit package version")
    if SEMVER.fullmatch(version) is None:
        raise ReleaseError(f"invalid Cargo package semantic version: {version!r}")
    return version


def validate_tag(manifest: Path, tag: str) -> tuple[str, bool]:
    version = package_version(manifest)
    if tag != f"v{version}":
        raise ReleaseError(f"release tag must be exactly v{version}; received {tag!r}")
    match = SEMVER.fullmatch(version)
    assert match is not None
    return version, match.group(4) is not None


def digest(path: Path) -> str:
    if path.is_symlink() or not path.is_file():
        raise ReleaseError(f"expected a regular, non-symlink file: {path}")
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_checksums(directory: Path, names: tuple[str, ...]) -> None:
    checksums = "".join(f"{digest(directory / name)}  {name}\n" for name in sorted(names))
    (directory / "SHA256SUMS").write_text(checksums, encoding="utf-8", newline="\n")


def verify_staged(directory: Path) -> dict:
    expected = set(STAGED) | {"SHA256SUMS"}
    if directory.is_symlink() or not directory.is_dir():
        raise ReleaseError("build artifact must be a non-symlink directory")
    if {path.name for path in directory.iterdir()} != expected:
        raise ReleaseError("build artifact contains missing or unexpected files")
    digest(directory / "SHA256SUMS")
    recorded: dict[str, str] = {}
    for line in (directory / "SHA256SUMS").read_text(encoding="utf-8").splitlines():
        match = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.-]+)", line)
        if match is None or match[2] not in STAGED or match[2] in recorded:
            raise ReleaseError("invalid, duplicate, or unexpected checksum entry")
        recorded[match[2]] = match[1]
    if set(recorded) != set(STAGED):
        raise ReleaseError("checksum manifest does not cover every build input")
    for name, checksum in recorded.items():
        if digest(directory / name) != checksum:
            raise ReleaseError(f"build artifact checksum mismatch: {name}")
    metadata = json.loads((directory / "BUILD-INFO.json").read_text(encoding="utf-8"))
    if not isinstance(metadata, dict):
        raise ReleaseError("build metadata must be a JSON object")
    return metadata


def command(args: list[str], root: Path) -> str:
    return subprocess.run(args, cwd=root, check=True, capture_output=True, text=True).stdout.strip()


def stage_build(root: Path, build_dir: Path, output: Path, objcopy: Path) -> None:
    version = package_version(root / "Cargo.toml")
    if command(["git", "status", "--porcelain", "--untracked-files=no"], root):
        raise ReleaseError("refusing to stage a build from a modified tracked source tree")
    commit = command(["git", "rev-parse", "HEAD"], root)
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ReleaseError("cannot determine the source commit")
    if os.environ.get("GITHUB_SHA", commit) != commit:
        raise ReleaseError("checked-out source does not match the workflow source commit")
    input_hashes = {name: digest(root / name) for name in INPUTS}
    identity = commit.encode()
    for name in ("bt2usb", "bt2usb-selftest"):
        if (build_dir / name).stat().st_size == 0:
            raise ReleaseError(f"empty firmware: {name}")
        digest(build_dir / name)
        # build.rs embeds the commit the boot log reports (src/diagnostics.rs),
        # with "-dirty" when tracked files differed: an image built from
        # other source must not be packaged under this commit.
        firmware = (build_dir / name).read_bytes()
        if identity not in firmware or identity + b"-dirty" in firmware:
            raise ReleaseError(f"{name} does not report the clean source commit {commit}")
    rustc = command(["rustc", "--version"], root)
    cargo = command(["cargo", "--version"], root)
    output.mkdir(parents=True, exist_ok=False)
    shutil.copyfile(build_dir / "bt2usb", output / "bt2usb.elf")
    shutil.copyfile(build_dir / "bt2usb-selftest", output / "bt2usb-selftest.elf")
    subprocess.run(
        [
            str(objcopy.resolve()),
            "-O",
            "ihex",
            str((output / "bt2usb.elf").resolve()),
            str((output / "bt2usb.hex").resolve()),
        ],
        check=True,
    )
    for name in INPUTS:
        shutil.copyfile(root / name, output / name)
    metadata = {
        "schema_version": 1,
        "package": "bt2usb",
        "version": version,
        "source_commit": commit,
        "source_ref": os.environ.get("GITHUB_REF", ""),
        "repository": os.environ.get("GITHUB_REPOSITORY", ""),
        "workflow_run_id": os.environ.get("GITHUB_RUN_ID", ""),
        "workflow_run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", ""),
        "target": TARGET,
        "profile": "release",
        "features": ["embedded"],
        "defmt_log": os.environ.get("DEFMT_LOG", "info"),
        "rustc": rustc,
        "cargo": cargo,
        "input_sha256": input_hashes,
        "artifact_sha256": {name: digest(output / name) for name in FIRMWARE},
    }
    (output / "BUILD-INFO.json").write_text(
        json.dumps(metadata, indent=2, sort_keys=True) + "\n", encoding="utf-8", newline="\n"
    )
    write_checksums(output, STAGED)


def package_release(
    root: Path,
    source: Path,
    output: Path,
    tag: str,
    expected_commit: str,
    expected_repository: str,
    expected_run_id: str,
) -> None:
    version, _ = validate_tag(root / "Cargo.toml", tag)
    metadata = verify_staged(source)
    if not re.fullmatch(r"[0-9a-f]{40}", expected_commit):
        raise ReleaseError("expected source commit must be a complete 40-character SHA")
    if not expected_repository or not expected_run_id.isdecimal():
        raise ReleaseError("expected repository and workflow run ID are required")
    expected = {
        "schema_version": 1,
        "package": "bt2usb",
        "version": version,
        "source_commit": expected_commit,
        "source_ref": f"refs/tags/{tag}",
        "repository": expected_repository,
        "workflow_run_id": expected_run_id,
        "target": TARGET,
        "profile": "release",
        "features": ["embedded"],
        "defmt_log": "info",
    }
    for field, value in expected.items():
        if metadata.get(field) != value:
            raise ReleaseError(f"build metadata does not match expected {field}")
    for name in INPUTS:
        checksum = digest(root / name)
        if (
            digest(source / name) != checksum
            or metadata.get("input_sha256", {}).get(name) != checksum
        ):
            raise ReleaseError(f"build input differs from release source: {name}")
    for name in FIRMWARE:
        if metadata.get("artifact_sha256", {}).get(name) != digest(source / name):
            raise ReleaseError(f"build metadata artifact digest mismatch: {name}")
    toolchain = tomllib.loads((root / "rust-toolchain.toml").read_text(encoding="utf-8"))
    channel = toolchain["toolchain"]["channel"]
    if not metadata.get("rustc", "").startswith(f"rustc {channel} "):
        raise ReleaseError("build compiler does not match the pinned Rust toolchain")
    names = {
        "bt2usb.elf": f"bt2usb-{tag}.elf",
        "bt2usb-selftest.elf": f"bt2usb-selftest-{tag}.elf",
        "bt2usb.hex": f"bt2usb-{tag}.hex",
        **{name: name for name in (*INPUTS, "BUILD-INFO.json")},
    }
    output.mkdir(parents=True, exist_ok=False)
    for original, renamed in names.items():
        shutil.copyfile(source / original, output / renamed)
    write_checksums(output, tuple(names.values()))


def rust_constant(path: Path, name: str) -> str:
    """The literal value of a top-level `const NAME: Type = value;` in `path`."""
    found = re.search(
        rf"^(?:pub )?const {name}: [^=]+= ([^;]+);$",
        path.read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if found is None:
        raise ReleaseError(f"{path.name} does not define {name}")
    return found[1]


def softdevice_prerequisite(root: Path) -> dict[str, str]:
    """The SoftDevice the linker script links against, which `mask softdevice` must install."""
    linker = (root / "memory_sd.x").read_text(encoding="utf-8")
    named = re.search(r"SoftDevice (S1[0-9]{2}) v([0-9]+\.[0-9]+\.[0-9]+)", linker)
    flash = re.search(r"^\s*FLASH : ORIGIN = (0x[0-9A-Fa-f]{8}),", linker, re.MULTILINE)
    ram = re.search(r"^\s*RAM : ORIGIN = (0x[0-9A-Fa-f]{8}),", linker, re.MULTILINE)
    if named is None or flash is None or ram is None:
        raise ReleaseError("memory_sd.x must name the SoftDevice and the FLASH and RAM origins")
    hex_file = f"{named[1].lower()}_nrf52_{named[2]}_softdevice.hex"
    if f'SD_HEX="{hex_file}"' not in (root / "maskfile.md").read_text(encoding="utf-8"):
        raise ReleaseError(
            f"mask softdevice does not install {hex_file}, which memory_sd.x expects"
        )
    return {
        "softdevice": f"{named[1]} v{named[2]}",
        "softdevice_hex": hex_file,
        "app_flash_start": flash[1],
        "app_ram_start": ram[1],
    }


def firmware_limits(root: Path) -> dict[str, str]:
    """Capacity, USB identity, and pairing-store layout compiled into the firmware."""
    config = root / "src" / "config.rs"
    framing = root / "src" / "storage" / "framing.rs"
    first = int(rust_constant(config, "STORAGE_FLASH_PAGE_START"))
    count = int(rust_constant(config, "STORAGE_FLASH_PAGE_COUNT"))
    page_size = int(rust_constant(config, "FLASH_PAGE_SIZE"))
    return {
        "max_paired": rust_constant(config, "MAX_PAIRED_DEVICES"),
        "max_connections": rust_constant(config, "BLE_MAX_CONNECTIONS"),
        "usb_vid": rust_constant(config, "USB_VID"),
        "usb_pid": rust_constant(config, "USB_PID"),
        "storage_pages": f"{first} to {first + count - 1}",
        "storage_start": f"0x{first * page_size:08X}",
        "storage_magic": rust_constant(framing, "MAGIC"),
        "storage_version": rust_constant(framing, "VERSION"),
    }


def release_notes(root: Path, package: Path, tag: str) -> str:
    """Fill the release notes template from a verified package and the tagged source."""
    version, prerelease = validate_tag(root / "Cargo.toml", tag)
    metadata = json.loads((package / "BUILD-INFO.json").read_text(encoding="utf-8"))
    if not isinstance(metadata, dict) or metadata.get("version") != version:
        raise ReleaseError("package build metadata does not match the release version")
    fields = ("source_commit", "repository", "workflow_run_id", "rustc", "defmt_log")
    if not all(isinstance(metadata.get(field), str) and metadata[field] for field in fields):
        raise ReleaseError("package build metadata lacks the build identity")
    commit = metadata["source_commit"]
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise ReleaseError("package build metadata has no complete source commit")
    checksums = (package / "SHA256SUMS").read_text(encoding="utf-8").rstrip("\n")
    if not checksums:
        raise ReleaseError("package checksum manifest is empty")
    for line in checksums.split("\n"):
        entry = re.fullmatch(r"([0-9a-f]{64})  ([A-Za-z0-9_.+-]+)", line)
        if entry is None or digest(package / entry[2]) != entry[1]:
            raise ReleaseError(f"package checksum entry does not match its file: {line!r}")
    template = string.Template((root / NOTES_TEMPLATE).read_text(encoding="utf-8"))
    return template.substitute(
        kind="Prerelease" if prerelease else "Release",
        tag=tag,
        version=version,
        commit=commit,
        short_commit=commit[:7],
        repository=metadata["repository"],
        run_id=metadata["workflow_run_id"],
        rustc=metadata["rustc"],
        defmt_log=metadata["defmt_log"],
        checksums=checksums,
        **softdevice_prerequisite(root),
        **firmware_limits(root),
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent)
    commands = parser.add_subparsers(dest="command", required=True)
    validate = commands.add_parser("validate-tag")
    validate.add_argument("--tag", required=True)
    stage = commands.add_parser("stage")
    stage.add_argument("--build-dir", type=Path, required=True)
    stage.add_argument("--output", type=Path, required=True)
    stage.add_argument("--objcopy", type=Path, required=True)
    package = commands.add_parser("package")
    package.add_argument("--input", type=Path, required=True)
    package.add_argument("--output", type=Path, required=True)
    package.add_argument("--tag", required=True)
    package.add_argument("--expected-commit", required=True)
    package.add_argument("--expected-repository", required=True)
    package.add_argument("--expected-run-id", required=True)
    notes = commands.add_parser("notes")
    notes.add_argument("--package", type=Path, required=True)
    notes.add_argument("--tag", required=True)
    notes.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    try:
        if args.command == "validate-tag":
            version, prerelease = validate_tag(args.root / "Cargo.toml", args.tag)
            if output_file := os.environ.get("GITHUB_OUTPUT"):
                with open(output_file, "a", encoding="utf-8", newline="\n") as handle:
                    handle.write(f"version={version}\nprerelease={str(prerelease).lower()}\n")
            print(f"Validated release tag {args.tag}")
        elif args.command == "stage":
            stage_build(args.root, args.build_dir, args.output, args.objcopy)
        elif args.command == "notes":
            text = release_notes(args.root, args.package, args.tag)
            args.output.parent.mkdir(parents=True, exist_ok=True)
            with args.output.open("x", encoding="utf-8", newline="\n") as handle:
                handle.write(text)
            print(f"Wrote release notes for {args.tag} to {args.output}")
        else:
            package_release(
                args.root,
                args.input,
                args.output,
                args.tag,
                args.expected_commit,
                args.expected_repository,
                args.expected_run_id,
            )
    except (
        ReleaseError,
        OSError,
        subprocess.CalledProcessError,
        ValueError,
        KeyError,
        TypeError,
    ) as error:
        parser.exit(1, f"Release check failed: {error}\n")


if __name__ == "__main__":
    main()
