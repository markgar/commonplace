"""Explicit spike-only acquisition; never called by tests or inference."""

import hashlib
import json
from pathlib import Path
import subprocess
import sys


def digest(data, kind):
    if kind == "sha256":
        return hashlib.sha256(data).hexdigest()
    if kind == "git-blob-sha1":
        return hashlib.sha1(f"blob {len(data)}\0".encode() + data).hexdigest()
    raise ValueError(f"unsupported digest: {kind}")


root = Path(sys.argv[1]).resolve()
manifest = json.loads(Path(__file__).with_name("models.json").read_text())
for model in manifest:
    for name, kind, expected in model["files"]:
        destination = root / model["revision"] / name
        if destination.exists():
            if digest(destination.read_bytes(), kind) != expected:
                raise ValueError(f"incompatible cached artifact: {destination}")
            continue
        destination.parent.mkdir(parents=True, exist_ok=True)
        partial = destination.with_suffix(destination.suffix + ".part")
        url = f"https://huggingface.co/{model['repository']}/resolve/{model['revision']}/{name}"
        print(url, flush=True)
        try:
            subprocess.run(
                ["curl", "--fail", "--location", "--show-error", "--silent",
                 "--max-time", "300", "--output", str(partial), url],
                check=True,
            )
            if digest(partial.read_bytes(), kind) != expected:
                raise ValueError(f"artifact checksum mismatch: {url}")
            partial.rename(destination)
        finally:
            partial.unlink(missing_ok=True)
print(f"Verified immutable cache: {root}")
