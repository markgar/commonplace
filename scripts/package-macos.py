#!/usr/bin/env python3
"""Build a local macOS arm64 package from clean source and existing pinned caches."""

import csv
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import tomllib


ROOT = Path(__file__).resolve().parent.parent
SANDBOX = ["sandbox-exec", "-p", "(version 1)(allow default)(deny network*)"]


def sha256(path):
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def require(condition, message):
    if not condition:
        raise SystemExit(message)


def run(args, env):
    return subprocess.check_output(args, cwd=ROOT, env=env, text=True).strip()


def main():
    require(platform.system() == "Darwin" and platform.machine() == "arm64",
            "only native macOS arm64 packaging is supported by this helper")
    require(os.environ.get("COMMONPLACE_MODEL_CACHE"),
            "set COMMONPLACE_MODEL_CACHE to an existing pinned cache")
    cache = Path(os.environ["COMMONPLACE_MODEL_CACHE"]).resolve(strict=True)
    env = {
        "HOME": os.environ["HOME"],
        "PATH": "/opt/homebrew/opt/rustup/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        "CARGO_TARGET_DIR": str(ROOT / "target"),
        "RUSTUP_AUTO_INSTALL": "0",
    }
    require(not run(["git", "status", "--porcelain", "--untracked-files=all"], env),
            "commit all source changes before packaging")
    commit = run(["git", "rev-parse", "HEAD"], env)
    tree = run(["git", "rev-parse", "HEAD^{tree}"], env)
    package = tomllib.loads((ROOT / "Cargo.toml").read_text())["package"]
    name = f"commonplace-{package['version']}-{commit[:12]}-macos-arm64"
    dist = ROOT / "target" / "dist"
    archive = dist / f"{name}.tar.gz"
    receipt = dist / f"{name}.tar.gz.sha256"
    require(not archive.exists() and not receipt.exists(), f"refusing to overwrite {archive}")

    models = json.loads((ROOT / "spikes/rust-packaging/models.json").read_text())
    model_hashes = {}
    for model in models:
        for filename, algorithm, expected in model["files"]:
            source = cache / model["revision"] / filename
            require(source.is_file(), f"missing pinned model: {source}")
            if algorithm == "sha256":
                actual = sha256(source)
            else:
                require(algorithm == "git-blob-sha1", f"unknown model hash: {algorithm}")
                data = source.read_bytes()
                actual = hashlib.sha1(b"blob " + str(len(data)).encode() + b"\0" + data).hexdigest()
            require(actual == expected, f"pinned model hash mismatch: {source}")
            model_hashes[(model["revision"], filename)] = sha256(source)

    metadata = json.loads(run(SANDBOX + [
        "cargo", "metadata", "--locked", "--offline", "--format-version", "1",
        "--filter-platform", "aarch64-apple-darwin",
    ], env))
    packages = {p["name"]: p for p in metadata["packages"]}
    ort_source = Path(packages["ort-sys"]["manifest_path"]).parent
    with (ort_source / "build/download/dist.tsv").open() as source:
        selected = [row for row in csv.DictReader(source, delimiter="\t")
                    if row["target"] == "aarch64-apple-darwin" and row["feature_set"] == "coreml"]
    require(len(selected) == 1, "cannot identify the pinned native ONNX distribution")
    native_dist = selected[0]
    native_archive = (Path(env["HOME"]) / "Library/Caches/ort.pyke.io/dfbin"
                      / native_dist["target"] / native_dist["sha256_hash"] / "libonnxruntime.a")
    require(native_archive.is_file(), f"missing cached native ONNX library: {native_archive}")
    native_hash = sha256(native_archive)
    dist.mkdir(parents=True, exist_ok=True)
    build_log = dist / f"{name}.build.log"
    require(not build_log.exists(), f"refusing to overwrite {build_log}")
    command = SANDBOX + ["cargo", "build", "--release", "--bin", "commonplace",
                         "--locked", "--offline"]
    print(f"Building {commit}; log: {build_log}", flush=True)
    with build_log.open("x") as log:
        subprocess.run(command, cwd=ROOT, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
    require(run(["git", "rev-parse", "HEAD"], env) == commit
            and not run(["git", "status", "--porcelain", "--untracked-files=all"], env),
            "source changed during packaging; refusing to label the artifact")
    require(sha256(native_archive) == native_hash, "native cache changed during build")

    binary = ROOT / "target/release/commonplace"
    require(run(["lipo", "-archs", str(binary)], env) == "arm64", "not an arm64 binary")
    linkage = run(["otool", "-L", str(binary)], env)
    dependencies = [line.strip().split(" (", 1)[0] for line in linkage.splitlines()[1:]]
    require(dependencies and all(path.startswith(("/usr/lib/", "/System/Library/"))
                                 for path in dependencies),
            f"unbundled non-system native dependency; stop for review:\n{linkage}")
    loads = run(["otool", "-l", str(binary)], env)
    for block in loads.split("Load command"):
        if "cmd LC_RPATH\n" in block:
            rpath = next(line.strip().split(" (", 1)[0][5:]
                         for line in block.splitlines() if line.strip().startswith("path "))
            require(rpath.startswith(("/usr/lib/", "/System/Library/")),
                    f"unexpected runtime search path: {rpath}")
    subprocess.run(["codesign", "--verify", "--strict", str(binary)], env=env, check=True)

    native_outputs = {}
    for output in sorted((ROOT / "target/release/build").glob("*/output")):
        if output.parent.name.startswith(("ort-sys-", "oxrocksdb-sys-", "libsqlite3-sys-", "sqlite-vec-")):
            native_outputs[output.parent.name] = output.read_text()
    require(any(str(native_archive.parent) in text and "static=onnxruntime" in text
                for key, text in native_outputs.items() if key.startswith("ort-sys-")),
            "build output does not confirm the identified static ONNX library")

    provenance = {
        "format": "commonplace-package-provenance/1",
        "package": {
            "name": package["name"],
            "version": package["version"],
        },
        "source_commit": commit,
        "source_tree": tree,
        "cargo_lock_sha256": sha256(ROOT / "Cargo.lock"),
        "target": "aarch64-apple-darwin",
        "host": run(["sw_vers"], env),
        "rustc": run(["rustc", "-Vv"], env),
        "cargo": run(["cargo", "-V"], env),
        "clang": run(["xcrun", "clang", "--version"], env),
        "sdk": run(["xcrun", "--show-sdk-version"], env),
        "build_command": command,
        "build_environment": env,
        "build_log_sha256": sha256(build_log),
        "native_onnx_distribution": native_dist,
        "native_onnx_static_library_sha256": native_hash,
        "native_cache_note": "Pre-existing extracted archive; original distribution was not reacquired.",
        "components": {key: {"version": packages[key]["version"], "license": packages[key]["license"]}
                       for key in ("oxigraph", "oxrocksdb-sys", "rusqlite", "libsqlite3-sys",
                                   "sqlite-vec", "fastembed", "ort", "ort-sys", "hf-hub")},
        "native_build_outputs": native_outputs,
        "mach_o_dependencies": dependencies,
        "mach_o_load_commands": loads,
        "model_acquisition": "Preseeded verified cache; no cold acquisition tested.",
        "release_status": "Local development candidate only. No independent clean-target proof, "
                          "signing/notarization, redistribution clearance or release approval.",
    }
    with tempfile.TemporaryDirectory(prefix="package-", dir=dist) as temporary:
        bundle = Path(temporary) / name
        bundle.mkdir()
        shutil.copy2(binary, bundle / "commonplace")
        shutil.copyfile(ROOT / "Cargo.lock", bundle / "Cargo.lock")
        shutil.copyfile(ROOT / "spikes/rust-packaging/models.json", bundle / "models.json")
        for model in models:
            for filename, _, _ in model["files"]:
                source = cache / model["revision"] / filename
                destination = bundle / "pinned-models" / model["revision"] / filename
                destination.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(source, destination)
                require(sha256(destination) == model_hashes[(model["revision"], filename)],
                        f"pinned model changed during packaging: {source}")
                destination.chmod(0o444)
        (bundle / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
        (bundle / "USAGE.txt").write_text(
            "Local macOS arm64 development package; not a published or notarized release.\n"
            "Use the repository-owned scripts/install-local-macos.py helper with the archive path.\n"
            "The helper verifies the archive sidecar, this bundle and the executable before installation.\n"
            "It may be copied alongside the archive; installation requires no GitHub access or gh.\n"
            "No Python, Cargo or compiler is used by the installed commonplace executable.\n\n"
            "For every terminal or local agent process, supply the existing model-cache environment:\n\n"
            'export PATH="$HOME/.local/bin:$PATH"\n'
            f'export COMMONPLACE_MODEL_CACHE="$HOME/.local/share/commonplace/releases/{name}/pinned-models"\n'
            'store="$HOME/.local/share/commonplace/stores/personal"\n'
            'commonplace --store "$store" init\n\n'
            "Or deliberately create the optional user path defaults once:\n\n"
            'config="$HOME/Library/Application Support/commonplace/config.json"\n'
            f'release_dir="$HOME/.local/share/commonplace/releases/{name}"\n'
            'store="$HOME/.local/share/commonplace/stores/personal"\n'
            'mkdir -p "$(dirname "$config")"\n'
            'tmp="$config.tmp"\n'
            'cat > "$tmp" <<EOF\n'
            '{\n'
            '  "format": "commonplace-user-config/1",\n'
            '  "store": "$store",\n'
            '  "model_cache": "$release_dir/pinned-models"\n'
            '}\n'
            'EOF\n'
            'mv "$tmp" "$config"\n'
            'commonplace config show\n'
            'commonplace init\n\n'
            "The package and install commands do not write this user-owned file.\n"
            "It is distinct from the selected store's backend-managed config.json.\n"
            "Keep the store outside the release directory. Installation neither copies nor removes stores.\n"
            "Retain the archive, checksum and provenance. Do not overwrite an installation implicitly.\n"
            "Use --help and the repository README for other public commands.\n"
            "Models are preseeded; no network is required. Missing/corrupt models fail explicitly.\n"
            "All five independent clean-target release gates remain unverified.\n"
        )
        inventory = sorted(path for path in bundle.rglob("*") if path.is_file())
        (bundle / "SHA256SUMS").write_text("".join(
            f"{sha256(path)}  {path.relative_to(bundle)}\n" for path in inventory
        ))
        require(run(["git", "rev-parse", "HEAD"], env) == commit
                and not run(["git", "status", "--porcelain", "--untracked-files=all"], env),
                "source changed during packaging")
        with tarfile.open(archive, "x:gz") as output:
            output.add(bundle, arcname=name)
    receipt.write_text(f"{sha256(archive)}  {archive.name}\n")
    print(archive)
    print(receipt.read_text(), end="")


if __name__ == "__main__":
    main()
