"""Build the release's CycloneDX software bill of materials (SBOM) from Cargo's
resolved dependency graph for the firmware build.

Python 3.11+; standard library only. `release.py stage` passes in `cargo tree`
and `cargo metadata` output for the embedded target and features, and the SBOM
it writes travels in the checksummed, attested release package. Every other
input is the build's own too: `Cargo.lock` for crate checksums, `deny.toml`
for the license of a crate that names only a license file, and the SoftDevice
the linker script expects. The output is deterministic, so a rebuild of the
same commit writes the same bytes.
"""

from __future__ import annotations

import re
import tomllib
import uuid
from urllib.parse import quote

SPEC_VERSION = "1.5"
CRATES_IO = "registry+https://github.com/rust-lang/crates.io-index"
FILE_NAME = "bt2usb.cdx.json"


def lock_checksums(lock_text: str) -> dict[tuple[str, str, str], str]:
    """The SHA-256 of each registry crate in `Cargo.lock`, by name, version, and source."""
    return {
        (package["name"], package["version"], package["source"]): package["checksum"]
        for package in tomllib.loads(lock_text).get("package", [])
        if "checksum" in package and "source" in package
    }


def license_clarifications(deny_text: str) -> dict[str, str]:
    """The license expression `deny.toml` assigns to each crate it clarifies."""
    clarify = tomllib.loads(deny_text).get("licenses", {}).get("clarify", [])
    return {entry["crate"]: entry["expression"] for entry in clarify}


def tree_graph(tree: str) -> tuple[tuple[str, str], dict[tuple[str, str], set]]:
    """The root and each package's dependencies in `cargo tree --prefix depth
    --format {p}` output, by name and version.

    `cargo tree`, unlike `cargo metadata`, resolves features the way the build
    does, so a crate that only another target's or a test's features would
    pull in is absent.
    """
    root = None
    children: dict[tuple[str, str], set] = {}
    stack: list[tuple[str, str]] = []
    for line in tree.splitlines():
        entry = re.match(r"([0-9]+)(\S+) v(\S+)", line)
        if entry is None or int(entry[1]) > len(stack) or (root is None) != (entry[1] == "0"):
            raise ValueError(f"unexpected cargo tree line: {line!r}")
        package = (entry[2], entry[3])
        del stack[int(entry[1]) :]
        if stack:
            children[stack[-1]].add(package)
        else:
            root = package
        children.setdefault(package, set())
        stack.append(package)
    if root is None:
        raise ValueError("cargo tree printed no packages")
    return root, children


def _relative(manifest_path: str, root_manifest: str) -> str:
    """A path dependency's directory relative to the repository root."""
    root = root_manifest.replace("\\", "/").rsplit("/", 1)[0]
    directory = manifest_path.replace("\\", "/").rsplit("/", 1)[0]
    if not directory.startswith(root + "/"):
        raise ValueError(f"path dependency outside the repository: {manifest_path}")
    return directory[len(root) + 1 :]


def build_sbom(
    metadata: dict,
    built: str,
    linked: str,
    checksums: dict[tuple[str, str, str], str],
    clarify: dict[str, str],
    *,
    version: str,
    commit: str,
    repository: str,
    target: str,
    features: list[str],
    softdevice: dict[str, str],
) -> dict:
    """The CycloneDX document for the firmware build.

    `built` is the `cargo tree` of everything the build compiles (normal and
    build dependencies) and `linked` of what the firmware links (normal
    dependencies without procedural macros); `metadata`, from `cargo
    metadata`, describes each package. A linked crate is `required`; one that
    only runs while building, a build script's dependency or a procedural
    macro, is `excluded`. Development dependencies are left out.
    """
    root, graph = tree_graph(built)
    linked_root, linked_graph = tree_graph(linked)
    if linked_root != root or not linked_graph.keys() <= graph.keys():
        raise ValueError("the linked dependency tree is not part of the built one")
    packages: dict[tuple[str, str], list[dict]] = {}
    for package in metadata["packages"]:
        packages.setdefault((package["name"], package["version"]), []).append(package)
    for key in graph:
        if len(packages.get(key, ())) != 1:
            raise ValueError(f"cargo metadata does not name exactly one {key[0]} {key[1]}")
    root_package = packages[root][0]
    tree = f"https://github.com/{repository}/tree/{commit}"

    refs = {root: "bt2usb"}
    components = []
    for key in sorted(graph.keys() - {root}):
        package = packages[key][0]
        (name, crate_version), source = key, package["source"]
        component = {"type": "library", "name": name, "version": crate_version}
        if source == CRATES_IO:
            ref = f"pkg:cargo/{name}@{crate_version}"
            checksum = checksums.get((name, crate_version, source))
            if checksum is None:
                raise ValueError(f"Cargo.lock has no checksum for {name} {crate_version}")
            component["hashes"] = [{"alg": "SHA-256", "content": checksum}]
        elif source is not None and source.startswith("git+"):
            url, _, revision = source[len("git+") :].partition("#")
            vcs = quote(f"git+{url.split('?')[0]}@{revision}", safe=":/")
            ref = f"pkg:cargo/{name}@{crate_version}?vcs_url={vcs}"
        elif source is None:
            path = _relative(package["manifest_path"], root_package["manifest_path"])
            ref = f"{name}@{crate_version}?path={path}"
            component["description"] = f"Vendored source at {path} in this repository"
            component["externalReferences"] = [{"type": "vcs", "url": f"{tree}/{path}"}]
        else:
            raise ValueError(f"unexpected source for {name} {crate_version}: {source}")
        if ref.startswith("pkg:"):
            component["purl"] = ref
        # Older crates write `MIT/Apache-2.0` for the SPDX `MIT OR Apache-2.0`.
        expression = package.get("license") or clarify.get(name)
        if expression:
            spdx = " OR ".join(part.strip() for part in expression.split("/"))
            component["licenses"] = [{"expression": spdx}]
        component["bom-ref"] = ref
        component["scope"] = "required" if key in linked_graph else "excluded"
        refs[key] = ref
        components.append(component)

    sd_name, _, sd_version = softdevice["softdevice"].partition(" v")
    sd_ref = f"nordic-{sd_name.lower()}@{sd_version}"
    components.append(
        {
            "type": "firmware",
            "bom-ref": sd_ref,
            "name": f"{sd_name.lower()}_nrf52",
            "version": sd_version,
            "supplier": {"name": "Nordic Semiconductor ASA"},
            "scope": "required",
            "description": (
                f"{softdevice['softdevice_hex']}, which the application runs on; not in "
                "this package, flashed separately with `mask softdevice`"
            ),
        }
    )
    depends = {refs[key]: {refs[dep] for dep in graph[key]} for key in graph}
    depends["bt2usb"].add(sd_ref)
    depends[sd_ref] = set()
    dependencies = [{"ref": ref, "dependsOn": sorted(depends[ref])} for ref in sorted(depends)]

    root_license = root_package.get("license")
    return {
        "bomFormat": "CycloneDX",
        "specVersion": SPEC_VERSION,
        "serialNumber": "urn:uuid:"
        + str(uuid.uuid5(uuid.NAMESPACE_URL, f"https://github.com/{repository}/commit/{commit}")),
        "version": 1,
        "metadata": {
            "component": {
                "type": "firmware",
                "bom-ref": "bt2usb",
                "name": "bt2usb",
                "version": version,
                **({"licenses": [{"expression": root_license}]} if root_license else {}),
                "externalReferences": [{"type": "vcs", "url": tree}],
            },
            "tools": {"components": [{"type": "application", "name": "scripts/release_sbom.py"}]},
            "properties": [
                {"name": "bt2usb:source_commit", "value": commit},
                {"name": "bt2usb:target", "value": target},
                {"name": "bt2usb:features", "value": ",".join(features)},
            ],
        },
        "components": components,
        "dependencies": dependencies,
    }


def sbom_identity(document: dict) -> tuple[str, str]:
    """The firmware version and source commit an SBOM describes."""
    metadata = document["metadata"]
    commits = [p["value"] for p in metadata["properties"] if p["name"] == "bt2usb:source_commit"]
    if len(commits) != 1:
        raise ValueError("SBOM must name exactly one source commit")
    return metadata["component"]["version"], commits[0]
