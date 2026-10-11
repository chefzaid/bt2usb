"""Release policy regression tests; no network, publishing, or firmware build."""

import json
import re
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest.mock import patch

import release
from docs_checks.links import anchors
from docs_checks.model import Document

REPOSITORY = Path(__file__).resolve().parent.parent
NOTES_SOURCES = (
    release.NOTES_TEMPLATE,
    Path("memory_sd.x"),
    Path("maskfile.md"),
    Path("src", "config.rs"),
    Path("src", "storage", "framing.rs"),
)


class ReleaseTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name) / "repo"
        self.root.mkdir()
        self.manifest = self.root / "Cargo.toml"
        self.set_version("0.1.0")
        (self.root / "Cargo.lock").write_text("version = 4\n", encoding="utf-8")
        (self.root / "rust-toolchain.toml").write_text(
            '[toolchain]\nchannel = "1.95.0"\n', encoding="utf-8"
        )
        self.source = Path(self.temp.name) / "input"
        self.source.mkdir()
        self.output = Path(self.temp.name) / "dist"
        self.commit = "a" * 40
        self.metadata = {
            "schema_version": 1,
            "package": "bt2usb",
            "version": "0.1.0",
            "source_commit": self.commit,
            "source_ref": "refs/tags/v0.1.0",
            "repository": "chefzaid/bt2usb",
            "workflow_run_id": "123",
            "workflow_run_attempt": "1",
            "target": release.TARGET,
            "profile": "release",
            "features": ["embedded"],
            "defmt_log": "info",
            "rustc": "rustc 1.95.0 (example)",
            "cargo": "cargo 1.95.0 (example)",
        }
        for name in release.INPUTS:
            (self.source / name).write_bytes((self.root / name).read_bytes())
        for name in release.FIRMWARE:
            (self.source / name).write_bytes(b"fixture payload: " + name.encode())
        self.metadata["input_sha256"] = {
            name: release.digest(self.source / name) for name in release.INPUTS
        }
        self.metadata["artifact_sha256"] = {
            name: release.digest(self.source / name) for name in release.FIRMWARE
        }
        self.refresh_metadata()

    def set_version(self, version):
        self.manifest.write_text(
            f'[package]\nname = "bt2usb"\nversion = "{version}"\n', encoding="utf-8"
        )

    def refresh_metadata(self):
        (self.source / "BUILD-INFO.json").write_text(json.dumps(self.metadata), encoding="utf-8")
        release.write_checksums(self.source, release.STAGED)

    def package(self):
        release.package_release(
            self.root, self.source, self.output, "v0.1.0", self.commit, "chefzaid/bt2usb", "123"
        )

    def test_exact_stable_prerelease_and_build_metadata_tags(self):
        for version, prerelease in (
            ("0.1.0", False),
            ("1.2.3-rc.1", True),
            ("1.2.3-0+build.42", True),
            ("1.2.3+build.42", False),
        ):
            with self.subTest(version=version):
                self.set_version(version)
                self.assertEqual(
                    release.validate_tag(self.manifest, f"v{version}"), (version, prerelease)
                )

    def test_prerelease_package_keeps_exact_version_identity(self):
        version = "0.2.0-rc.1+build.42"
        self.set_version(version)
        (self.source / "Cargo.toml").write_bytes(self.manifest.read_bytes())
        self.metadata["version"] = version
        self.metadata["source_ref"] = f"refs/tags/v{version}"
        self.metadata["input_sha256"]["Cargo.toml"] = release.digest(self.manifest)
        self.refresh_metadata()
        release.package_release(
            self.root,
            self.source,
            self.output,
            f"v{version}",
            self.commit,
            "chefzaid/bt2usb",
            "123",
        )
        self.assertTrue((self.output / f"bt2usb-v{version}.hex").is_file())

    def test_wrong_tag_or_nonsemantic_version_fails(self):
        for tag in ("0.1.0", "v0.1.1", "v0.1.0-rc.1", "v0.1.0\n", "v0.1.0/../../escape"):
            with self.subTest(tag=tag), self.assertRaises(release.ReleaseError):
                release.validate_tag(self.manifest, tag)
        for version in ("01.2.3", "1.2", "1.2.3-01", "1.2.3-", "1.2.3+", "1.2.3/evil"):
            self.set_version(version)
            with self.subTest(version=version), self.assertRaises(release.ReleaseError):
                release.validate_tag(self.manifest, f"v{version}")

    def test_package_preserves_exact_built_payload_and_checksums(self):
        self.package()
        for original, renamed in (
            ("bt2usb.elf", "bt2usb-v0.1.0.elf"),
            ("bt2usb-selftest.elf", "bt2usb-selftest-v0.1.0.elf"),
            ("bt2usb.hex", "bt2usb-v0.1.0.hex"),
        ):
            self.assertEqual(
                (self.source / original).read_bytes(), (self.output / renamed).read_bytes()
            )
        for line in (self.output / "SHA256SUMS").read_text().splitlines():
            checksum, name = line.split("  ")
            self.assertEqual(checksum, release.digest(self.output / name))
        self.assertEqual(
            (self.output / "BUILD-INFO.json").read_bytes(),
            (self.source / "BUILD-INFO.json").read_bytes(),
        )

    def test_artifact_tampering_is_rejected(self):
        (self.source / "bt2usb.elf").write_bytes(b"modified after successful build")
        with self.assertRaisesRegex(release.ReleaseError, "checksum mismatch"):
            self.package()
        self.assertFalse(self.output.exists())

    def test_metadata_identity_and_build_policy_must_match(self):
        for field, wrong in (
            ("source_commit", "b" * 40),
            ("source_ref", "refs/heads/main"),
            ("repository", "attacker/fork"),
            ("workflow_run_id", "999"),
            ("version", "0.2.0"),
            ("target", "host"),
            ("profile", "debug"),
            ("features", ["sim"]),
            ("defmt_log", "debug"),
            ("schema_version", 2),
            ("rustc", "rustc 1.96.0 (different)"),
        ):
            with self.subTest(field=field):
                original = self.metadata[field]
                self.metadata[field] = wrong
                self.refresh_metadata()
                with self.assertRaises(release.ReleaseError):
                    self.package()
                self.metadata[field] = original
                self.assertFalse(self.output.exists())

    def test_changed_lockfile_is_rejected(self):
        (self.root / "Cargo.lock").write_text("different resolution", encoding="utf-8")
        with self.assertRaisesRegex(release.ReleaseError, "build input differs"):
            self.package()

    def test_rehashed_artifact_must_still_match_build_metadata(self):
        (self.source / "bt2usb.hex").write_bytes(b"different bytes")
        release.write_checksums(self.source, release.STAGED)
        with self.assertRaisesRegex(release.ReleaseError, "artifact digest mismatch"):
            self.package()

    def test_missing_extra_duplicate_and_unsafe_checksum_entries_fail(self):
        manifest = self.source / "SHA256SUMS"
        original = manifest.read_text()
        bad = [
            original.splitlines()[0] + "\n",
            original + original.splitlines()[0] + "\n",
            original + "0" * 64 + "  ../escape\n",
        ]
        for text in bad:
            manifest.write_text(text, encoding="utf-8")
            with self.subTest(text=text), self.assertRaises(release.ReleaseError):
                self.package()
        manifest.write_text(original, encoding="utf-8")
        (self.source / "extra.elf").write_bytes(b"unexpected")
        with self.assertRaisesRegex(release.ReleaseError, "unexpected files"):
            self.package()

    def test_existing_destination_is_not_overwritten(self):
        self.output.mkdir()
        marker = self.output / "keep.txt"
        marker.write_text("preserve", encoding="utf-8")
        with self.assertRaises(FileExistsError):
            self.package()
        self.assertEqual(marker.read_text(), "preserve")

    def test_staging_records_exact_compiled_bytes_and_build_identity(self):
        build = Path(self.temp.name) / "build"
        build.mkdir()
        (build / "bt2usb").write_bytes(b"compiled application " + self.commit.encode())
        (build / "bt2usb-selftest").write_bytes(b"compiled selftest " + self.commit.encode())
        output = Path(self.temp.name) / "stage"
        environment = {
            "GITHUB_SHA": self.commit,
            "GITHUB_REF": "refs/tags/v0.1.0",
            "GITHUB_REPOSITORY": "chefzaid/bt2usb",
            "GITHUB_RUN_ID": "123",
            "GITHUB_RUN_ATTEMPT": "1",
            "DEFMT_LOG": "info",
        }

        def convert(args, **kwargs):
            self.assertEqual(args[1:3], ["-O", "ihex"])
            Path(args[-1]).write_bytes(b"converted hex")

        with (
            patch.dict(release.os.environ, environment, clear=True),
            patch.object(
                release,
                "command",
                side_effect=["", self.commit, "rustc 1.95.0 (example)", "cargo 1.95.0 (example)"],
            ),
            patch.object(release.subprocess, "run", side_effect=convert),
        ):
            release.stage_build(self.root, build, output, Path("llvm-objcopy"))
        metadata = release.verify_staged(output)
        self.assertEqual(metadata["source_commit"], self.commit)
        self.assertEqual(
            metadata["artifact_sha256"]["bt2usb.elf"], release.digest(build / "bt2usb")
        )
        release.package_release(
            self.root, output, self.output, "v0.1.0", self.commit, "chefzaid/bt2usb", "123"
        )

    def test_staging_refuses_firmware_that_reports_another_or_dirty_commit(self):
        build = Path(self.temp.name) / "build"
        build.mkdir()
        commit = self.commit.encode()
        for application in (b"compiled application " + b"c" * 40, b"build " + commit + b"-dirty"):
            (build / "bt2usb").write_bytes(application)
            (build / "bt2usb-selftest").write_bytes(b"compiled selftest " + commit)
            with (
                self.subTest(application=application),
                patch.dict(release.os.environ, {"GITHUB_SHA": self.commit}),
                patch.object(release, "command", side_effect=["", self.commit]),
                self.assertRaisesRegex(release.ReleaseError, "bt2usb does not report"),
            ):
                release.stage_build(self.root, build, self.output, Path("llvm-objcopy"))
            self.assertFalse(self.output.exists())

    def test_staging_refuses_dirty_source_or_wrong_checkout(self):
        for results in ([" M src/main.rs"], ["", "b" * 40]):
            with (
                self.subTest(results=results),
                patch.dict(release.os.environ, {"GITHUB_SHA": self.commit}),
                patch.object(release, "command", side_effect=results),
                self.assertRaises(release.ReleaseError),
            ):
                release.stage_build(self.root, self.source, self.output, Path("llvm-objcopy"))
            self.assertFalse(self.output.exists())

    def notes(self, tag="v0.1.0"):
        for name in NOTES_SOURCES:
            if not (self.root / name).exists():
                (self.root / name).parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(REPOSITORY / name, self.root / name)
        return release.release_notes(self.root, self.output, tag)

    def test_notes_fill_every_field_from_the_package_and_source(self):
        self.package()
        notes = self.notes()
        self.assertNotIn("${", notes)
        self.assertIn("**bt2usb 0.1.0** (Release)", notes)
        self.assertIn(f"/commit/{self.commit})", notes)
        self.assertIn("/actions/runs/123)", notes)
        self.assertIn("**S140 v7.3.0**\n(`s140_nrf52_7.3.0_softdevice.hex`)", notes)
        self.assertIn("`0x00027000` in flash and `0x20006000` in RAM", notes)
        self.assertIn("at most\n  2 connected at once and 4 saved", notes)
        self.assertIn("VID `0x1209` and PID `0x0001`", notes)
        self.assertIn("pages 240 to 243 (from\n`0x000F0000`)", notes)
        self.assertIn("magic `0xB2` and\nstorage version `0x01`", notes)
        self.assertIn((self.output / "SHA256SUMS").read_text(encoding="utf-8"), notes)
        self.assertIn('--source-digest "$approved_commit"', notes)
        self.assertEqual(len(re.findall(r"^(?:- )?REVIEW: ", notes, re.MULTILINE)), 4)

    def test_notes_links_reach_existing_guide_headings(self):
        self.package()
        links = re.findall(r"/blob/v0\.1\.0/(docs/[a-z-]+\.md)#([a-z0-9-]+)\)", self.notes())
        self.assertGreaterEqual(len(links), 6)
        for path, fragment in links:
            with self.subTest(link=f"{path}#{fragment}"):
                text = (REPOSITORY / path).read_text(encoding="utf-8")
                self.assertIn(fragment, anchors(Document(path, text)))

    def test_prerelease_notes_say_so(self):
        version = "0.2.0-rc.1"
        self.set_version(version)
        self.output.mkdir()
        (self.output / "BUILD-INFO.json").write_text(
            json.dumps({**self.metadata, "version": version}), encoding="utf-8"
        )
        release.write_checksums(self.output, ("BUILD-INFO.json",))
        self.assertIn(f"**bt2usb {version}** (Prerelease)", self.notes(f"v{version}"))

    def test_notes_refuse_a_package_that_does_not_match(self):
        self.package()
        checksums = self.output / "SHA256SUMS"
        original = checksums.read_text(encoding="utf-8")
        for changed in (
            original.replace("  bt2usb-v0.1.0.hex", "  missing.hex"),
            original.replace("  bt2usb-v0.1.0.hex", "  ../bt2usb-v0.1.0.hex"),
            "0" * 64 + "  bt2usb-v0.1.0.hex\n",
            "",
        ):
            checksums.write_text(changed, encoding="utf-8")
            with self.subTest(checksums=changed), self.assertRaises(release.ReleaseError):
                self.notes()
        checksums.write_text(original, encoding="utf-8")
        metadata = self.output / "BUILD-INFO.json"
        for field, value in (("version", "0.0.9"), ("source_commit", "abc"), ("rustc", None)):
            metadata.write_text(json.dumps({**self.metadata, field: value}), encoding="utf-8")
            with self.subTest(field=field), self.assertRaises(release.ReleaseError):
                self.notes()

    def test_notes_require_the_softdevice_mask_installs(self):
        self.package()
        self.notes()
        maskfile = self.root / "maskfile.md"
        maskfile.write_text(
            maskfile.read_text(encoding="utf-8").replace("7.3.0_softdevice", "7.2.0_softdevice"),
            encoding="utf-8",
        )
        with self.assertRaisesRegex(release.ReleaseError, "mask softdevice"):
            self.notes()


if __name__ == "__main__":
    unittest.main()
