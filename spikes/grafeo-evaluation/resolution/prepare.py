#!/usr/bin/env python3
"""Reproduce the disposable patch from verified, already-cached Grafeo sources."""

import argparse
import difflib
import hashlib
import json
from pathlib import Path
import subprocess
import tarfile

ROOT = Path(__file__).resolve().parent
PACKAGES = {
    "grafeo-common-0.5.43": "780d7fb460139e5ba9d6062d679893b24c724129a174710bd62fd45dc51157d4",
    "grafeo-core-0.5.43": "2c746c01145ab3c1a3a368c15012e1fdfc608c5d6cd27e158330c12d1f2d730e",
    "grafeo-engine-0.5.43": "d6beb48085a410e875eb52dec6c7a18ad6e5e1e650ea21c457ccf40624120ca5",
}


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--cache", required=True, type=Path,
                        help="Existing Cargo registry cache/index.* directory; never downloads")
    parser.add_argument("--capture", action="store_true",
                        help="Maintainer only: regenerate patch/manifest from target/vendor")
    args = parser.parse_args()
    vendor = ROOT / "target/vendor"
    patch = ROOT / "engine.patch"
    manifest_path = ROOT / "patch-manifest.json"
    sources = {}
    for package, expected in PACKAGES.items():
        archive = args.cache / f"{package}.crate"
        if digest(archive) != expected:
            raise RuntimeError(f"incompatible cached package: {package}")
        sources[package] = archive

    if args.capture:
        chunks, files = [], []
        for package, archive in sources.items():
            with tarfile.open(archive) as tar:
                for member in sorted(tar.getmembers(), key=lambda item: item.name):
                    if not member.isfile():
                        continue
                    path = member.name
                    original = tar.extractfile(member).read()
                    modified = vendor / path
                    before = hashlib.sha256(original).hexdigest()
                    if before == digest(modified):
                        continue
                    if not path.endswith(".rs"):
                        raise RuntimeError(f"unexpected non-Rust delta: {path}")
                    delta = list(difflib.unified_diff(
                        original.decode().splitlines(keepends=True),
                        modified.read_text().splitlines(keepends=True),
                        fromfile=f"a/{path}", tofile=f"b/{path}",
                        n=0,
                    ))
                    chunks.extend(delta)
                    files.append({
                        "path": path, "before_sha256": before, "after_sha256": digest(modified),
                        "added": sum(line.startswith("+") and not line.startswith("+++") for line in delta),
                        "removed": sum(line.startswith("-") and not line.startswith("---") for line in delta),
                    })
        patch.write_text("".join(chunks))
        manifest_path.write_text(json.dumps({
            "packages": PACKAGES, "patch_sha256": digest(patch), "files": files,
        }, indent=2) + "\n")
        print(f"Captured {len(files)} changed files; +{sum(f['added'] for f in files)} "
              f"-{sum(f['removed'] for f in files)}")
        return

    manifest = json.loads(manifest_path.read_text())
    if manifest["packages"] != PACKAGES or digest(patch) != manifest["patch_sha256"]:
        raise RuntimeError("patch identity mismatch")
    if vendor.exists():
        raise RuntimeError("target/vendor already exists; preserve it or remove that exact disposable directory explicitly")
    vendor.mkdir(parents=True)
    for archive in sources.values():
        with tarfile.open(archive) as tar:
            tar.extractall(vendor, filter="data")
    for entry in manifest["files"]:
        if digest(vendor / entry["path"]) != entry["before_sha256"]:
            raise RuntimeError(f"baseline mismatch: {entry['path']}")
    command = ["patch", "-p1", "--batch", "--forward", "-i", str(patch)]
    subprocess.run(command + ["--dry-run"], cwd=vendor, check=True)
    subprocess.run(command, cwd=vendor, check=True)
    for entry in manifest["files"]:
        if digest(vendor / entry["path"]) != entry["after_sha256"]:
            raise RuntimeError(f"patched source mismatch: {entry['path']}")
    print("Prepared verified LOCAL SPIKE sources; production dependencies unchanged.")


if __name__ == "__main__":
    main()
