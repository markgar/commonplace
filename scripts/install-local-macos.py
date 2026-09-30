#!/usr/bin/env python3
"""Install one verified Commonplace macOS arm64 package for the current user."""

from __future__ import annotations

import datetime
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import platform
import re
import shutil
import stat
import subprocess
import sys
import tarfile
import tempfile
from typing import Callable


PROVENANCE_FORMAT = "commonplace-package-provenance/1"
RECEIPT_FORMAT = "commonplace-installed-release/1"
TARGET = "aarch64-apple-darwin"
TOP_LEVEL_COMMANDS = (
    "init",
    "graph",
    "ingest",
    "search",
    "record",
    "remove",
    "withdraw",
    "get",
    "schema",
)
RECEIPT_FIELDS = {
    "format",
    "package_version",
    "source_commit",
    "release_id",
    "release_path",
    "executable_path",
    "executable_sha256",
    "archive_sha256",
    "installed_at",
    "platform",
    "architecture",
}
MANUAL_RECEIPT_FORMAT = "commonplace-manual-install/1"
MANUAL_RECEIPT_FIELDS = {
    "format",
    "package_version",
    "source_commit",
    "source_tree",
    "main_commit",
    "release_id",
    "release_path",
    "executable_path",
    "executable_sha256",
    "installed_at",
    "platform",
    "architecture",
}
PENDING_FORMAT = "commonplace-installed-release-pending/1"
PENDING_FIELDS = {
    "format",
    "package_version",
    "source_commit",
    "release_id",
    "release_path",
    "executable_path",
    "executable_sha256",
    "archive_sha256",
}
SHA256_RE = re.compile(r"[0-9a-f]{64}")
COMMIT_RE = re.compile(r"[0-9a-f]{40}")
SIDECAR_RE = re.compile(r"([0-9a-f]{64})  ([^/\n]+)\n")
CHECKSUM_RE = re.compile(r"([0-9a-f]{64})  ([^\n]+)")
VERSION_RE = re.compile(r"[0-9A-Za-z][0-9A-Za-z.+-]*")


class InstallerError(Exception):
    """A verified, actionable installation failure."""


class PartialInstallError(InstallerError):
    """The stable executable changed but the receipt was not verified."""


def require(condition: bool, message: str) -> None:
    if not condition:
        raise InstallerError(message)


def sha256(path: Path) -> str:
    with path.open("rb") as source:
        return hashlib.file_digest(source, "sha256").hexdigest()


def lstat_or_none(path: Path) -> os.stat_result | None:
    try:
        return path.lstat()
    except FileNotFoundError:
        return None


def require_regular_file(path: Path, description: str) -> os.stat_result:
    metadata = lstat_or_none(path)
    require(metadata is not None, f"{description} is missing: {path}")
    require(stat.S_ISREG(metadata.st_mode), f"{description} is not a regular file: {path}")
    return metadata


def require_directory(path: Path, description: str) -> os.stat_result:
    metadata = lstat_or_none(path)
    require(metadata is not None, f"{description} is missing: {path}")
    require(stat.S_ISDIR(metadata.st_mode), f"{description} is not a directory: {path}")
    return metadata


def ensure_directory(path: Path, mode: int = 0o755) -> None:
    missing: list[Path] = []
    current = path
    while lstat_or_none(current) is None:
        missing.append(current)
        parent = current.parent
        require(parent != current, f"cannot create directory without an existing parent: {path}")
        current = parent
    require_directory(current, "installation parent")
    for directory in reversed(missing):
        directory.mkdir(mode=mode)
    require_directory(path, "installation directory")


def fsync_directory(path: Path) -> None:
    flags = os.O_RDONLY | getattr(os, "O_DIRECTORY", 0)
    descriptor = os.open(path, flags)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def normalize_relative_posix(name: str, description: str) -> PurePosixPath:
    require(name != "", f"{description} has an empty path")
    require("\\" not in name, f"{description} uses a backslash-ambiguous path: {name}")
    require(not name.startswith("/"), f"{description} uses an absolute path: {name}")
    path = PurePosixPath(name)
    require(not path.is_absolute(), f"{description} uses an absolute path: {name}")
    require(all(part not in ("", ".", "..") for part in path.parts),
            f"{description} uses an unsafe path: {name}")
    require(str(path) == name.rstrip("/"), f"{description} uses a non-canonical path: {name}")
    return path


def archive_release_id(archive: Path) -> str:
    suffix = ".tar.gz"
    require(archive.name.endswith(suffix), f"archive must end in {suffix}: {archive}")
    release_id = archive.name[:-len(suffix)]
    require(release_id != "", f"archive has no release identifier: {archive}")
    return release_id


def verify_archive_sidecar(archive: Path) -> str:
    require(archive.is_absolute(), f"archive path must be absolute: {archive}")
    require_regular_file(archive, "archive")
    sidecar = Path(f"{archive}.sha256")
    require_regular_file(sidecar, "archive checksum sidecar")
    try:
        text = sidecar.read_text(encoding="ascii")
    except (OSError, UnicodeError) as error:
        raise InstallerError(f"cannot read archive checksum sidecar {sidecar}: {error}") from error
    match = SIDECAR_RE.fullmatch(text)
    require(match is not None, f"archive checksum sidecar has an invalid format: {sidecar}")
    expected, filename = match.groups()
    require(filename == archive.name,
            f"archive checksum sidecar names {filename}, expected {archive.name}")
    actual = sha256(archive)
    require(actual == expected,
            f"archive checksum mismatch for {archive}: expected {expected}, got {actual}")
    return actual


def validate_tar_members(
    archive: tarfile.TarFile,
    release_id: str,
) -> list[tuple[tarfile.TarInfo, PurePosixPath]]:
    validated: list[tuple[tarfile.TarInfo, PurePosixPath]] = []
    seen: set[str] = set()
    top_levels: set[str] = set()
    for member in archive.getmembers():
        path = normalize_relative_posix(member.name, "archive member")
        normalized = str(path)
        require(normalized not in seen, f"archive contains a duplicate path: {normalized}")
        seen.add(normalized)
        top_levels.add(path.parts[0])
        require(member.isdir() or member.isreg(),
                f"archive contains a link or special member: {normalized}")
        require(not getattr(member, "sparse", None),
                f"archive contains an unsupported sparse member: {normalized}")
        if path == PurePosixPath(release_id) / "commonplace":
            require(member.isreg() and member.mode & stat.S_IXUSR != 0,
                    "archive executable is not a regular executable file")
        validated.append((member, path))
    require(validated, "archive is empty")
    require(top_levels == {release_id},
            f"archive must contain exactly one top-level directory named {release_id}")
    require(any(path == PurePosixPath(release_id) and member.isdir()
                for member, path in validated),
            f"archive is missing its top-level directory entry: {release_id}")
    return validated


def extract_archive(archive_path: Path, release_id: str, destination: Path) -> Path:
    try:
        with tarfile.open(archive_path, "r:gz") as archive:
            members = validate_tar_members(archive, release_id)
            for member, relative in members:
                output = destination.joinpath(*relative.parts)
                require(output == destination / Path(*relative.parts),
                        f"archive member escapes extraction root: {relative}")
                if member.isdir():
                    output.mkdir(parents=True, exist_ok=True, mode=0o700)
                    require_directory(output, "extracted directory")
                    continue
                output.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
                source = archive.extractfile(member)
                require(source is not None, f"cannot read archive member: {relative}")
                descriptor = os.open(output, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
                written = 0
                try:
                    with os.fdopen(descriptor, "wb") as target:
                        while True:
                            chunk = source.read(1024 * 1024)
                            if not chunk:
                                break
                            target.write(chunk)
                            written += len(chunk)
                        target.flush()
                        os.fsync(target.fileno())
                finally:
                    source.close()
                require(written == member.size,
                        f"archive member size mismatch for {relative}: expected {member.size}, got {written}")
    except (tarfile.TarError, OSError) as error:
        raise InstallerError(f"cannot safely extract archive {archive_path}: {error}") from error
    bundle = destination / release_id
    require_directory(bundle, "extracted bundle")
    return bundle


def parse_checksums(bundle: Path) -> dict[PurePosixPath, str]:
    checksum_path = bundle / "SHA256SUMS"
    require_regular_file(checksum_path, "bundle checksum inventory")
    try:
        text = checksum_path.read_text(encoding="ascii")
    except (OSError, UnicodeError) as error:
        raise InstallerError(f"cannot read bundle checksum inventory {checksum_path}: {error}") from error
    require(text.endswith("\n"), f"bundle checksum inventory lacks a final newline: {checksum_path}")
    checksums: dict[PurePosixPath, str] = {}
    for line in text.splitlines():
        match = CHECKSUM_RE.fullmatch(line)
        require(match is not None, f"invalid SHA256SUMS line: {line!r}")
        digest, name = match.groups()
        path = normalize_relative_posix(name, "bundle checksum")
        require(path != PurePosixPath("SHA256SUMS"),
                "SHA256SUMS must not contain a checksum for itself")
        require(path not in checksums, f"duplicate SHA256SUMS path: {path}")
        checksums[path] = digest
    require(checksums, "bundle checksum inventory is empty")
    return checksums


def bundle_files(bundle: Path) -> set[PurePosixPath]:
    files: set[PurePosixPath] = set()
    for path in bundle.rglob("*"):
        metadata = path.lstat()
        relative = PurePosixPath(path.relative_to(bundle).as_posix())
        require(not stat.S_ISLNK(metadata.st_mode), f"bundle contains a symbolic link: {relative}")
        require(stat.S_ISREG(metadata.st_mode) or stat.S_ISDIR(metadata.st_mode),
                f"bundle contains a special file: {relative}")
        if stat.S_ISREG(metadata.st_mode):
            files.add(relative)
    return files


def bundle_directories(bundle: Path) -> set[PurePosixPath]:
    directories: set[PurePosixPath] = set()
    for path in bundle.rglob("*"):
        metadata = path.lstat()
        relative = PurePosixPath(path.relative_to(bundle).as_posix())
        require(not stat.S_ISLNK(metadata.st_mode), f"bundle contains a symbolic link: {relative}")
        require(stat.S_ISREG(metadata.st_mode) or stat.S_ISDIR(metadata.st_mode),
                f"bundle contains a special file: {relative}")
        if stat.S_ISDIR(metadata.st_mode):
            directories.add(relative)
    return directories


def required_directories(files: set[PurePosixPath]) -> set[PurePosixPath]:
    directories: set[PurePosixPath] = set()
    for path in files:
        parent = path.parent
        while parent != PurePosixPath("."):
            directories.add(parent)
            parent = parent.parent
    return directories


def validate_provenance(bundle: Path, release_id: str) -> dict[str, object]:
    path = bundle / "provenance.json"
    require_regular_file(path, "package provenance")
    try:
        provenance = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise InstallerError(f"invalid package provenance at {path}: {error}") from error
    require(isinstance(provenance, dict), f"package provenance is not an object: {path}")
    require(provenance.get("format") == PROVENANCE_FORMAT,
            f"unsupported package provenance format: {provenance.get('format')!r}")
    package = provenance.get("package")
    require(isinstance(package, dict) and set(package) == {"name", "version"},
            "package provenance must contain only package name and version")
    require(package["name"] == "commonplace", f"unexpected package name: {package['name']!r}")
    version = package["version"]
    require(isinstance(version, str) and VERSION_RE.fullmatch(version) is not None,
            f"invalid package version: {version!r}")
    source_commit = provenance.get("source_commit")
    require(isinstance(source_commit, str) and COMMIT_RE.fullmatch(source_commit) is not None,
            f"invalid package source commit: {source_commit!r}")
    source_tree = provenance.get("source_tree")
    require(isinstance(source_tree, str) and COMMIT_RE.fullmatch(source_tree) is not None,
            f"invalid package source tree: {source_tree!r}")
    require(provenance.get("target") == TARGET,
            f"package target is not {TARGET}: {provenance.get('target')!r}")
    expected_release_id = f"commonplace-{version}-{source_commit[:12]}-macos-arm64"
    require(release_id == expected_release_id,
            f"release identifier {release_id} does not match provenance {expected_release_id}")
    return provenance


def run_command(args: list[str]) -> subprocess.CompletedProcess[str]:
    try:
        return subprocess.run(args, check=False, capture_output=True, text=True)
    except OSError as error:
        raise InstallerError(f"cannot run {args[0]}: {error}") from error


def verify_native_executable(executable: Path) -> None:
    metadata = require_regular_file(executable, "package executable")
    require(metadata.st_mode & stat.S_IXUSR != 0,
            f"package executable is not executable: {executable}")
    architecture = run_command(["/usr/bin/lipo", "-archs", str(executable)])
    require(architecture.returncode == 0,
            f"cannot inspect executable architecture: {architecture.stderr.strip()}")
    require(architecture.stdout.strip() == "arm64",
            f"package executable is not native arm64: {architecture.stdout.strip()!r}")
    signature = run_command(["/usr/bin/codesign", "--verify", "--strict", str(executable)])
    require(signature.returncode == 0,
            f"package executable signature verification failed: {signature.stderr.strip()}")


def smoke_executable(executable: Path, version: str) -> None:
    version_result = run_command([str(executable), "--version"])
    require(version_result.returncode == 0,
            f"installed executable --version failed: {version_result.stderr.strip()}")
    require(version_result.stdout.strip() == f"commonplace {version}",
            f"installed executable reported an unexpected version: {version_result.stdout.strip()!r}")
    help_result = run_command([str(executable), "--help"])
    require(help_result.returncode == 0,
            f"installed executable --help failed: {help_result.stderr.strip()}")
    missing = [command for command in TOP_LEVEL_COMMANDS
               if re.search(rf"(?m)^  {re.escape(command)}(?:\s|$)", help_result.stdout) is None]
    require(not missing, f"installed executable help is missing commands: {', '.join(missing)}")


def verify_bundle(bundle: Path, release_id: str) -> tuple[dict[str, object], dict[PurePosixPath, str]]:
    checksums = parse_checksums(bundle)
    files = bundle_files(bundle)
    require(files == set(checksums) | {PurePosixPath("SHA256SUMS")},
            "bundle files do not exactly match SHA256SUMS")
    required = {
        PurePosixPath("commonplace"),
        PurePosixPath("Cargo.lock"),
        PurePosixPath("models.json"),
        PurePosixPath("provenance.json"),
        PurePosixPath("USAGE.txt"),
    }
    require(required.issubset(checksums), "bundle checksum inventory is missing required files")
    require(any(len(path.parts) > 1 and path.parts[0] == "pinned-models" for path in checksums),
            "bundle contains no pinned model files")
    require(bundle_directories(bundle) == required_directories(files),
            "bundle contains an unexpected or missing directory")
    for relative, expected in checksums.items():
        path = bundle.joinpath(*relative.parts)
        require_regular_file(path, "checksummed bundle file")
        actual = sha256(path)
        require(actual == expected,
                f"bundle checksum mismatch for {relative}: expected {expected}, got {actual}")
    provenance = validate_provenance(bundle, release_id)
    verify_native_executable(bundle / "commonplace")
    version = provenance["package"]["version"]  # type: ignore[index]
    smoke_executable(bundle / "commonplace", version)
    return provenance, checksums


def verify_platform() -> None:
    require(platform.system() == "Darwin" and platform.machine() == "arm64",
            "only native macOS arm64 installation is supported")


def immutable_mode(relative: PurePosixPath) -> int:
    return 0o555 if relative == PurePosixPath("commonplace") else 0o444


def verify_release_matches(
    release: Path,
    bundle: Path,
    checksums: dict[PurePosixPath, str],
) -> None:
    require_directory(release, "installed release")
    require(stat.S_IMODE(release.stat().st_mode) == 0o555,
            f"installed release directory has an unexpected mode: {release}")
    incoming_files = bundle_files(bundle)
    installed_files = bundle_files(release)
    require(installed_files == incoming_files, f"installed release inventory conflicts: {release}")
    require(bundle_directories(release) == bundle_directories(bundle),
            f"installed release directory inventory conflicts: {release}")
    for relative in sorted(incoming_files, key=str):
        incoming = bundle.joinpath(*relative.parts)
        installed = release.joinpath(*relative.parts)
        require(sha256(installed) == sha256(incoming),
                f"installed release conflicts at {installed}")
        expected_mode = immutable_mode(relative)
        require(stat.S_IMODE(installed.stat().st_mode) == expected_mode,
                f"installed release has an unexpected mode at {installed}")
    for path in release.rglob("*"):
        if path.is_dir():
            require(stat.S_IMODE(path.stat().st_mode) == 0o555,
                    f"installed release directory has an unexpected mode: {path}")
    provenance, installed_checksums = verify_bundle(release, release.name)
    del provenance
    require(installed_checksums == checksums, f"installed release checksum inventory conflicts: {release}")


def copy_release(bundle: Path, release: Path, checksums: dict[PurePosixPath, str]) -> None:
    releases = release.parent
    staging = Path(tempfile.mkdtemp(prefix=f".{release.name}.installing-", dir=releases))
    published = False
    try:
        directories = [path for path in bundle.rglob("*") if path.is_dir()]
        for directory in sorted(directories, key=lambda path: len(path.parts)):
            relative = directory.relative_to(bundle)
            (staging / relative).mkdir(mode=0o700)
        for relative in sorted(bundle_files(bundle), key=str):
            source = bundle.joinpath(*relative.parts)
            destination = staging.joinpath(*relative.parts)
            destination.parent.mkdir(parents=True, exist_ok=True, mode=0o700)
            descriptor = os.open(destination, os.O_WRONLY | os.O_CREAT | os.O_EXCL, 0o600)
            with source.open("rb") as input_file, os.fdopen(descriptor, "wb") as output_file:
                shutil.copyfileobj(input_file, output_file, 1024 * 1024)
                output_file.flush()
                os.fsync(output_file.fileno())
            destination.chmod(immutable_mode(relative))
        require(bundle_files(staging) == bundle_files(bundle),
                "staged release inventory changed during copy")
        for relative in sorted(bundle_files(staging), key=str):
            expected = sha256(bundle.joinpath(*relative.parts))
            require(sha256(staging.joinpath(*relative.parts)) == expected,
                    f"staged release checksum mismatch: {relative}")
        for directory in sorted(
            [staging, *[path for path in staging.rglob("*") if path.is_dir()]],
            key=lambda path: len(path.parts),
            reverse=True,
        ):
            directory.chmod(0o555)
            fsync_directory(directory)
        os.rename(staging, release)
        published = True
        fsync_directory(releases)
        verify_release_matches(release, bundle, checksums)
    except OSError as error:
        raise InstallerError(f"failed to publish immutable release {release}: {error}") from error
    finally:
        if not published and staging.exists():
            for path in sorted(staging.rglob("*"), key=lambda item: len(item.parts), reverse=True):
                if path.is_dir():
                    path.chmod(0o700)
            staging.chmod(0o700)
            shutil.rmtree(staging, ignore_errors=True)


def read_receipt_json(path: Path) -> tuple[str, dict[str, object]]:
    require_regular_file(path, "installed release receipt")
    try:
        text = path.read_text(encoding="utf-8")
        receipt = json.loads(text)
    except (OSError, UnicodeError, json.JSONDecodeError) as error:
        raise InstallerError(f"installed release receipt is invalid at {path}: {error}") from error
    require(isinstance(receipt, dict), f"installed release receipt is not an object: {path}")
    return text, receipt


def require_canonical_receipt_text(path: Path, text: str, receipt: dict[str, object]) -> None:
    canonical = json.dumps(receipt, indent=2, sort_keys=True) + "\n"
    require(text == canonical, f"installed release receipt is not canonical JSON: {path}")


def validate_receipt_timestamp(value: str) -> None:
    try:
        parsed_time = datetime.datetime.strptime(value, "%Y-%m-%dT%H:%M:%SZ")
    except ValueError as error:
        raise InstallerError("installed release receipt has an invalid installed_at") from error
    require(parsed_time.tzinfo is None, "installed release receipt has an invalid installed_at")


def strict_canonical_receipt(
    path: Path,
    text: str,
    receipt: dict[str, object],
) -> dict[str, str]:
    require(set(receipt) == RECEIPT_FIELDS,
            f"installed release receipt has unknown or missing fields: {path}")
    require(all(isinstance(receipt[field], str) for field in RECEIPT_FIELDS),
            f"installed release receipt fields must all be strings: {path}")
    require(receipt["format"] == RECEIPT_FORMAT,
            f"unsupported installed release receipt format: {receipt['format']!r}")
    require(VERSION_RE.fullmatch(receipt["package_version"]) is not None,
            "installed release receipt has an invalid package version")
    require(COMMIT_RE.fullmatch(receipt["source_commit"]) is not None,
            "installed release receipt has an invalid source commit")
    require(SHA256_RE.fullmatch(receipt["executable_sha256"]) is not None,
            "installed release receipt has an invalid executable hash")
    require(SHA256_RE.fullmatch(receipt["archive_sha256"]) is not None,
            "installed release receipt has an invalid archive hash")
    require(receipt["platform"] == "macos" and receipt["architecture"] == "arm64",
            "installed release receipt has an incompatible platform")
    expected_release_id = (
        f"commonplace-{receipt['package_version']}-{receipt['source_commit'][:12]}-macos-arm64"
    )
    require(receipt["release_id"] == expected_release_id,
            "installed release receipt has an invalid release identifier")
    validate_receipt_timestamp(receipt["installed_at"])
    require_canonical_receipt_text(path, text, receipt)
    return receipt  # type: ignore[return-value]


def strict_manual_receipt(
    path: Path,
    text: str,
    receipt: dict[str, object],
) -> dict[str, str]:
    require(set(receipt) == MANUAL_RECEIPT_FIELDS,
            f"manual installed release receipt has unknown or missing fields: {path}")
    require(all(isinstance(receipt[field], str) for field in MANUAL_RECEIPT_FIELDS),
            f"manual installed release receipt fields must all be strings: {path}")
    require(receipt["format"] == MANUAL_RECEIPT_FORMAT,
            f"unsupported manual installed release receipt format: {receipt['format']!r}")
    require(VERSION_RE.fullmatch(receipt["package_version"]) is not None,
            "manual installed release receipt has an invalid package version")
    for field in ("source_commit", "source_tree", "main_commit"):
        require(COMMIT_RE.fullmatch(receipt[field]) is not None,
                f"manual installed release receipt has an invalid {field}")
    require(SHA256_RE.fullmatch(receipt["executable_sha256"]) is not None,
            "manual installed release receipt has an invalid executable hash")
    require(receipt["platform"] == "macos" and receipt["architecture"] == "arm64",
            "manual installed release receipt has an incompatible platform")
    expected_release_id = (
        f"commonplace-{receipt['package_version']}-{receipt['source_commit'][:12]}"
        "-manual-macos-arm64"
    )
    require(receipt["release_id"] == expected_release_id,
            "manual installed release receipt has an invalid release identifier")
    validate_receipt_timestamp(receipt["installed_at"])
    require_canonical_receipt_text(path, text, receipt)
    return receipt  # type: ignore[return-value]


def installed_receipt(path: Path) -> dict[str, str]:
    text, receipt = read_receipt_json(path)
    receipt_format = receipt.get("format")
    if receipt_format == RECEIPT_FORMAT:
        return strict_canonical_receipt(path, text, receipt)
    if receipt_format == MANUAL_RECEIPT_FORMAT:
        return strict_manual_receipt(path, text, receipt)
    raise InstallerError(f"unsupported installed release receipt format: {receipt_format!r}")


def installed_pending(path: Path) -> dict[str, str]:
    text, pending = read_receipt_json(path)
    require(set(pending) == PENDING_FIELDS,
            f"pending installed release marker has unknown or missing fields: {path}")
    require(all(isinstance(pending[field], str) for field in PENDING_FIELDS),
            f"pending installed release marker fields must all be strings: {path}")
    require(pending["format"] == PENDING_FORMAT,
            f"unsupported pending installed release marker format: {pending['format']!r}")
    require(VERSION_RE.fullmatch(pending["package_version"]) is not None,
            "pending installed release marker has an invalid package version")
    require(COMMIT_RE.fullmatch(pending["source_commit"]) is not None,
            "pending installed release marker has an invalid source commit")
    require(SHA256_RE.fullmatch(pending["executable_sha256"]) is not None,
            "pending installed release marker has an invalid executable hash")
    require(SHA256_RE.fullmatch(pending["archive_sha256"]) is not None,
            "pending installed release marker has an invalid archive hash")
    expected_release_id = (
        f"commonplace-{pending['package_version']}-{pending['source_commit'][:12]}-macos-arm64"
    )
    require(pending["release_id"] == expected_release_id,
            "pending installed release marker has an invalid release identifier")
    require_canonical_receipt_text(path, text, pending)
    return pending  # type: ignore[return-value]


def verify_manual_release(receipt: dict[str, str], releases_root: Path) -> None:
    release = releases_root / receipt["release_id"]
    require(Path(receipt["release_path"]) == release,
            f"manual installed receipt names an invalid release path: {receipt['release_path']}")
    require_directory(release, "manual installed release")
    entries = list(release.iterdir())
    require(len(entries) == 1 and entries[0].name == "commonplace",
            f"manual installed release has an unexpected inventory: {release}")
    executable = entries[0]
    metadata = require_regular_file(executable, "manual release executable")
    require(metadata.st_mode & stat.S_IXUSR != 0,
            f"manual release executable is not executable: {executable}")
    require(sha256(executable) == receipt["executable_sha256"],
            f"manual release executable hash does not match its receipt: {executable}")


def current_installed_state(
    receipt_path: Path,
    executable_path: Path,
    incoming_executable_hash: str,
) -> tuple[dict[str, str] | None, str]:
    receipt_metadata = lstat_or_none(receipt_path)
    executable_metadata = lstat_or_none(executable_path)
    if receipt_metadata is None:
        if executable_metadata is None:
            return None, "first-install"
        require(stat.S_ISREG(executable_metadata.st_mode),
                f"stable executable is not a regular file: {executable_path}")
        current_hash = sha256(executable_path)
        if current_hash == incoming_executable_hash:
            return None, "recover-first-install"
        raise InstallerError(
            f"refusing to replace unmanaged executable at {executable_path}; its SHA-256 is {current_hash}"
        )
    receipt = installed_receipt(receipt_path)
    require(executable_metadata is not None and stat.S_ISREG(executable_metadata.st_mode),
            f"installed receipt exists but the stable executable is missing or invalid: {executable_path}")
    require(Path(receipt["executable_path"]) == executable_path,
            f"installed receipt names a different executable: {receipt['executable_path']}")
    current_hash = sha256(executable_path)
    if current_hash == receipt["executable_sha256"]:
        return receipt, "coherent"
    if current_hash == incoming_executable_hash:
        return receipt, "recover-update"
    raise InstallerError(
        f"stable executable hash {current_hash} matches neither the receipt nor the incoming archive"
    )


def receipt_identity_matches(
    receipt: dict[str, str],
    expected: dict[str, str],
) -> bool:
    return all(receipt[field] == expected[field] for field in RECEIPT_FIELDS - {"installed_at"})


def activate_executable(
    source: Path,
    destination: Path,
    expected_hash: str,
    replace: Callable[[Path, Path], None],
) -> None:
    descriptor, name = tempfile.mkstemp(prefix=".commonplace.installing-", dir=destination.parent)
    staging = Path(name)
    try:
        with source.open("rb") as input_file, os.fdopen(descriptor, "wb") as output_file:
            shutil.copyfileobj(input_file, output_file, 1024 * 1024)
            output_file.flush()
            os.fsync(output_file.fileno())
        staging.chmod(0o755)
        require(sha256(staging) == expected_hash, "staged stable executable checksum mismatch")
        replace(staging, destination)
    except OSError as error:
        raise InstallerError(f"failed to activate stable executable {destination}: {error}") from error
    finally:
        if staging.exists():
            staging.unlink()


def timestamp_now() -> str:
    return datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0).strftime(
        "%Y-%m-%dT%H:%M:%SZ"
    )


def publish_receipt(
    receipt_path: Path,
    receipt: dict[str, str],
    replace: Callable[[Path, Path], None],
) -> None:
    bytes_value = (json.dumps(receipt, indent=2, sort_keys=True) + "\n").encode()
    descriptor, name = tempfile.mkstemp(prefix=".installed-release.json.installing-",
                                        dir=receipt_path.parent)
    staging = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(bytes_value)
            output.flush()
            os.fsync(output.fileno())
        staging.chmod(0o644)
        replace(staging, receipt_path)
        fsync_directory(receipt_path.parent)
    except OSError as error:
        raise InstallerError(f"failed to publish installed release receipt {receipt_path}: {error}") from error
    finally:
        if staging.exists():
            staging.unlink()


def publish_pending(
    pending_path: Path,
    pending: dict[str, str],
    replace: Callable[[Path, Path], None],
) -> None:
    bytes_value = (json.dumps(pending, indent=2, sort_keys=True) + "\n").encode()
    descriptor, name = tempfile.mkstemp(prefix=".installed-release.pending.json.installing-",
                                        dir=pending_path.parent)
    staging = Path(name)
    try:
        with os.fdopen(descriptor, "wb") as output:
            output.write(bytes_value)
            output.flush()
            os.fsync(output.fileno())
        staging.chmod(0o644)
        replace(staging, pending_path)
        fsync_directory(pending_path.parent)
    except OSError as error:
        raise InstallerError(
            f"failed to publish pending installed release marker {pending_path}: {error}"
        ) from error
    finally:
        if staging.exists():
            staging.unlink()


def remove_pending(pending_path: Path) -> None:
    require_regular_file(pending_path, "pending installed release marker")
    try:
        pending_path.unlink()
        fsync_directory(pending_path.parent)
    except OSError as error:
        raise InstallerError(
            f"failed to clear pending installed release marker {pending_path}: {error}"
        ) from error


def partial_guidance(
    archive: Path,
    executable: Path,
    receipt: Path,
    pending: Path,
    release: Path,
    error: Exception,
) -> PartialInstallError:
    script = Path(__file__).resolve()
    return PartialInstallError(
        "the stable executable may have been activated but the installed release receipt was not verified; "
        f"installation is partial: {error}. Inspect with "
        f"`shasum -a 256 {executable}`, `cat {receipt}` (if present), and "
        f"`cat {pending}`; verify release `{release}`, then retry the same archive with "
        f"`python3 {script} {archive}`."
    )


def install_archive(
    archive: Path,
    home: Path,
    *,
    replace: Callable[[Path, Path], None] = os.replace,
    now: Callable[[], str] = timestamp_now,
    check_platform: bool = True,
) -> tuple[str, Path, Path]:
    if check_platform:
        verify_platform()
    require(home.is_absolute(), f"HOME must be absolute: {home}")
    require_directory(home, "HOME")
    release_id = archive_release_id(archive)
    archive_hash = verify_archive_sidecar(archive)
    with tempfile.TemporaryDirectory(prefix="commonplace-install-") as temporary:
        bundle = extract_archive(archive, release_id, Path(temporary))
        (bundle / "commonplace").chmod(0o700)
        provenance, checksums = verify_bundle(bundle, release_id)
        version = provenance["package"]["version"]  # type: ignore[index]
        source_commit = provenance["source_commit"]
        incoming_executable_hash = checksums[PurePosixPath("commonplace")]

        share_root = home / ".local" / "share" / "commonplace"
        releases_root = share_root / "releases"
        bin_root = home / ".local" / "bin"
        release = releases_root / release_id
        executable = bin_root / "commonplace"
        receipt_path = share_root / "installed-release.json"
        pending_path = share_root / "installed-release.pending.json"
        ensure_directory(releases_root)
        ensure_directory(bin_root)

        expected_identity = {
            "format": RECEIPT_FORMAT,
            "package_version": version,
            "source_commit": source_commit,
            "release_id": release_id,
            "release_path": str(release),
            "executable_path": str(executable),
            "executable_sha256": incoming_executable_hash,
            "archive_sha256": archive_hash,
            "platform": "macos",
            "architecture": "arm64",
        }
        expected_pending = {
            "format": PENDING_FORMAT,
            "package_version": version,
            "source_commit": source_commit,
            "release_id": release_id,
            "release_path": str(release),
            "executable_path": str(executable),
            "executable_sha256": incoming_executable_hash,
            "archive_sha256": archive_hash,
        }
        pending = (
            installed_pending(pending_path)
            if lstat_or_none(pending_path) is not None
            else None
        )
        if pending is not None:
            require(
                pending == expected_pending,
                "another installation is pending at "
                f"{pending_path} for release {pending['release_id']} and archive SHA-256 "
                f"{pending['archive_sha256']}; retry that same verified archive before "
                f"installing {release_id}",
            )
        receipt, state = current_installed_state(
            receipt_path,
            executable,
            incoming_executable_hash,
        )
        if state in ("recover-first-install", "recover-update"):
            require(
                pending is not None,
                f"stable executable recovery state has no verified pending marker at {pending_path}",
            )
        if receipt is not None:
            expected_prior_release = releases_root / receipt["release_id"]
            require(Path(receipt["release_path"]) == expected_prior_release,
                    f"installed receipt names an invalid release path: {receipt['release_path']}")
            if receipt["format"] == MANUAL_RECEIPT_FORMAT:
                verify_manual_release(receipt, releases_root)
                if state == "coherent":
                    smoke_executable(executable, receipt["package_version"])
            if state == "recover-update":
                require(receipt["executable_sha256"] != incoming_executable_hash,
                        "installed receipt is inconsistent with its stable executable")

        if release.exists():
            verify_release_matches(release, bundle, checksums)
        else:
            copy_release(bundle, release, checksums)

        if (
            receipt is not None
            and receipt["format"] == RECEIPT_FORMAT
            and state == "coherent"
            and receipt_identity_matches(receipt, expected_identity)
        ):
            try:
                require(sha256(executable) == incoming_executable_hash,
                        "idempotent reinstall found a changed stable executable")
                smoke_executable(executable, version)
                fsync_directory(executable.parent)
                fsync_directory(receipt_path.parent)
                fsync_directory(release.parent)
                if pending is not None:
                    remove_pending(pending_path)
            except (InstallerError, OSError) as error:
                raise partial_guidance(
                    archive, executable, receipt_path, pending_path, release, error
                ) from error
            return "unchanged", executable, receipt_path

        smoke_executable(release / "commonplace", version)
        if pending is None:
            publish_pending(pending_path, expected_pending, replace)
        if state not in ("recover-first-install", "recover-update"):
            try:
                activate_executable(
                    release / "commonplace",
                    executable,
                    incoming_executable_hash,
                    replace,
                )
            except InstallerError as error:
                raise InstallerError(
                    f"{error}. Pending installation is recorded at {pending_path}; "
                    f"retry the same verified archive {archive}"
                ) from error

        try:
            fsync_directory(executable.parent)
            require(sha256(executable) == incoming_executable_hash,
                    "stable executable checksum changed after activation")
            smoke_executable(executable, version)
            new_receipt = dict(expected_identity)
            new_receipt["installed_at"] = now()
            publish_receipt(receipt_path, new_receipt, replace)
            final_receipt = installed_receipt(receipt_path)
            require(final_receipt == new_receipt,
                    "installed release receipt changed after publication")
            verify_release_matches(release, bundle, checksums)
            require(sha256(executable) == incoming_executable_hash,
                    "stable executable checksum changed after receipt publication")
            remove_pending(pending_path)
        except (InstallerError, OSError) as error:
            raise partial_guidance(
                archive, executable, receipt_path, pending_path, release, error
            ) from error
        return "installed" if receipt is None else "updated", executable, receipt_path


def main() -> int:
    if len(sys.argv) != 2:
        print(
            "usage: python3 scripts/install-local-macos.py "
            "/absolute/path/commonplace-<version>-<commit>-macos-arm64.tar.gz",
            file=sys.stderr,
        )
        return 2
    try:
        state, executable, receipt = install_archive(
            Path(sys.argv[1]),
            Path.home(),
        )
    except InstallerError as error:
        print(f"installation failed: {error}", file=sys.stderr)
        return 1
    print(f"{state}: {executable}")
    print(f"receipt: {receipt}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
