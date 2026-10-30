"""SBOM builder regression tests on a small fixture graph; no Cargo or network."""

import copy
import json
import unittest

import release_sbom

COMMIT = "a" * 40
GIT = "git+https://github.com/embassy-rs/nrf-softdevice?rev=47d6#47d6121c"
REGISTRY = release_sbom.CRATES_IO
LOCK = "\n".join(
    f'[[package]]\nname = "{name}"\nversion = "{version}"\nsource = "{REGISTRY}"\n'
    f'checksum = "{name[0] * 64}"\n'
    for name, version in (
        ("cc", "1.0.0"),
        ("hash32", "0.3.1"),
        ("heapless", "0.8.0"),
        ("macro-crate", "1.0.0"),
        ("proptest", "1.0.0"),
        ("syn", "2.0.0"),
    )
)
DENY = """
[[licenses.clarify]]
crate = "nrf-softdevice-s140"
expression = "LicenseRef-Nordic-5-Clause"
license-files = []
"""
# `cargo tree --prefix depth --format {p}`: everything built, then what links.
BUILT = """0bt2usb v0.1.0 (/repo)
1cc v1.0.0
1heapless v0.8.0
2hash32 v0.3.1
1macro-crate v1.0.0 (proc-macro)
2syn v2.0.0
1nrf-softdevice v0.1.0 (/repo/vendor/nrf-softdevice)
2heapless v0.8.0 (*)
2nrf-softdevice-s140 v0.1.2 (https://github.com/embassy-rs/nrf-softdevice?rev=47d6#47d6121c)
"""
LINKED = """0bt2usb v0.1.0 (/repo)
1heapless v0.8.0
2hash32 v0.3.1
1nrf-softdevice v0.1.0 (/repo/vendor/nrf-softdevice)
2heapless v0.8.0 (*)
2nrf-softdevice-s140 v0.1.2 (https://github.com/embassy-rs/nrf-softdevice?rev=47d6#47d6121c)
"""


def package(name, version, source=REGISTRY, license="MIT OR Apache-2.0", path=None):
    manifest = f"/repo/{path}/Cargo.toml" if path else f"/registry/{name}/Cargo.toml"
    return {
        "name": name,
        "version": version,
        "id": f"{name}@{version}",
        "source": source,
        "license": license,
        "manifest_path": manifest,
    }


METADATA = {
    "packages": [
        {**package("bt2usb", "0.1.0", None, "GPL-3.0-only"), "manifest_path": "/repo/Cargo.toml"},
        package("cc", "1.0.0"),
        package("hash32", "0.3.1", license="MIT/Apache-2.0"),
        package("heapless", "0.8.0"),
        package("macro-crate", "1.0.0"),
        package("nrf-softdevice", "0.1.0", None, path="vendor/nrf-softdevice"),
        package("nrf-softdevice-s140", "0.1.2", GIT, license=None),
        # A development dependency `cargo metadata` lists but no build compiles.
        package("proptest", "1.0.0"),
        package("syn", "2.0.0"),
    ]
}
SOFTDEVICE = {"softdevice": "S140 v7.3.0", "softdevice_hex": "s140_nrf52_7.3.0_softdevice.hex"}


def build(metadata=METADATA, built=BUILT, linked=LINKED, lock=LOCK, commit=COMMIT):
    return release_sbom.build_sbom(
        metadata,
        built,
        linked,
        release_sbom.lock_checksums(lock),
        release_sbom.license_clarifications(DENY),
        version="0.1.0",
        commit=commit,
        repository="chefzaid/bt2usb",
        target="thumbv7em-none-eabihf",
        features=["embedded"],
        softdevice=SOFTDEVICE,
    )


class SbomTests(unittest.TestCase):
    def setUp(self):
        self.sbom = build()
        self.components = {c["name"]: c for c in self.sbom["components"]}

    def test_linked_crates_are_required_and_build_time_crates_excluded(self):
        scopes = {name: c["scope"] for name, c in self.components.items()}
        self.assertEqual(
            scopes,
            {
                "cc": "excluded",
                "hash32": "required",
                "heapless": "required",
                "macro-crate": "excluded",
                "nrf-softdevice": "required",
                "nrf-softdevice-s140": "required",
                "s140_nrf52": "required",
                "syn": "excluded",
            },
        )

    def test_each_source_kind_is_identified_and_licensed(self):
        heapless = self.components["heapless"]
        self.assertEqual(heapless["purl"], "pkg:cargo/heapless@0.8.0")
        self.assertEqual(heapless["bom-ref"], heapless["purl"])
        self.assertEqual(heapless["hashes"], [{"alg": "SHA-256", "content": "h" * 64}])
        self.assertEqual(
            self.components["hash32"]["licenses"], [{"expression": "MIT OR Apache-2.0"}]
        )
        s140 = self.components["nrf-softdevice-s140"]
        self.assertEqual(
            s140["purl"],
            "pkg:cargo/nrf-softdevice-s140@0.1.2"
            "?vcs_url=git%2Bhttps://github.com/embassy-rs/nrf-softdevice%4047d6121c",
        )
        self.assertEqual(s140["licenses"], [{"expression": "LicenseRef-Nordic-5-Clause"}])
        self.assertNotIn("hashes", s140)
        vendored = self.components["nrf-softdevice"]
        self.assertEqual(vendored["bom-ref"], "nrf-softdevice@0.1.0?path=vendor/nrf-softdevice")
        self.assertNotIn("purl", vendored)
        self.assertEqual(
            vendored["externalReferences"],
            [
                {
                    "type": "vcs",
                    "url": f"https://github.com/chefzaid/bt2usb/tree/{COMMIT}/vendor/nrf-softdevice",
                }
            ],
        )
        softdevice = self.components["s140_nrf52"]
        self.assertEqual((softdevice["type"], softdevice["version"]), ("firmware", "7.3.0"))
        self.assertIn("s140_nrf52_7.3.0_softdevice.hex", softdevice["description"])

    def test_dependencies_follow_the_built_graph(self):
        depends = {d["ref"]: d["dependsOn"] for d in self.sbom["dependencies"]}
        refs = {c["bom-ref"] for c in self.sbom["components"]} | {"bt2usb"}
        self.assertEqual(set(depends), refs)
        self.assertEqual(len(depends), len(self.sbom["dependencies"]))
        self.assertEqual(
            depends["bt2usb"],
            [
                "nordic-s140@7.3.0",
                "nrf-softdevice@0.1.0?path=vendor/nrf-softdevice",
                "pkg:cargo/cc@1.0.0",
                "pkg:cargo/heapless@0.8.0",
                "pkg:cargo/macro-crate@1.0.0",
            ],
        )
        self.assertEqual(
            depends["nrf-softdevice@0.1.0?path=vendor/nrf-softdevice"],
            ["pkg:cargo/heapless@0.8.0", self.components["nrf-softdevice-s140"]["purl"]],
        )
        self.assertEqual(depends["pkg:cargo/macro-crate@1.0.0"], ["pkg:cargo/syn@2.0.0"])

    def test_document_identifies_the_build_and_is_deterministic(self):
        self.assertEqual(release_sbom.sbom_identity(self.sbom), ("0.1.0", COMMIT))
        self.assertEqual(
            self.sbom["metadata"]["component"]["licenses"][0]["expression"], "GPL-3.0-only"
        )
        self.assertEqual(json.dumps(build(), sort_keys=True), json.dumps(self.sbom, sort_keys=True))
        self.assertNotEqual(build(commit="b" * 40)["serialNumber"], self.sbom["serialNumber"])
        self.assertNotIn("timestamp", self.sbom["metadata"])

    def test_inconsistent_inputs_are_refused(self):
        ambiguous = copy.deepcopy(METADATA)
        ambiguous["packages"].append(package("heapless", "0.8.0", GIT))
        cases = {
            "missing checksum": {"lock": LOCK.replace('checksum = "' + "h" * 64 + '"\n', "")},
            "ambiguous package": {"metadata": ambiguous},
            "unknown package": {"built": BUILT + "1unknown v9.9.9\n"},
            "malformed line": {"built": BUILT + "not a tree line\n"},
            "depth jump": {"built": BUILT + "4hash32 v0.3.1\n"},
            "second root": {"built": BUILT + "0bt2usb v0.1.0 (/repo)\n"},
            "empty tree": {"built": ""},
            "linked outside built": {"linked": LINKED + "1proptest v1.0.0\n"},
        }
        outside = copy.deepcopy(METADATA)
        outside["packages"][5]["manifest_path"] = "/elsewhere/nrf-softdevice/Cargo.toml"
        cases["path outside the repository"] = {"metadata": outside}
        for case, arguments in cases.items():
            with self.subTest(case=case), self.assertRaises(ValueError):
                build(**arguments)
        with self.assertRaises(ValueError):
            release_sbom.sbom_identity({"metadata": {"component": {}, "properties": []}})


if __name__ == "__main__":
    unittest.main()
