import hashlib
import importlib.util
import io
import json
import os
from pathlib import Path
import shutil
import tarfile
import tempfile
import unittest
from unittest import mock


SCRIPT = Path(__file__).resolve().parents[1] / "install-local-macos.py"
SPEC = importlib.util.spec_from_file_location("install_local_macos", SCRIPT)
installer = importlib.util.module_from_spec(SPEC)
assert SPEC.loader is not None
SPEC.loader.exec_module(installer)


COMMANDS = ("config", "init", "graph", "ingest", "search", "record", "remove",
            "withdraw", "get", "schema")


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def executable_text(version, marker, commands=COMMANDS):
    commands = "".join(f"  {command} command\n" for command in commands)
    return f"""#!/bin/sh
# {marker}
case "$1" in
  --version)
    printf '%s\\n' 'commonplace {version}'
    ;;
  --help)
    cat <<'EOF'
Usage: commonplace <COMMAND>
Commands:
{commands}EOF
    ;;
  *)
    exit 2
    ;;
esac
"""


def make_package(root, *, version="0.1.0", commit_char="a", executable_marker=None,
                 extra_file=False, extra_directory=False, missing_usage=False,
                 bad_bundle_checksum=False):
    commit = commit_char * 40
    release_id = f"commonplace-{version}-{commit[:12]}-macos-arm64"
    bundle = root / release_id
    (bundle / "pinned-models" / "revision").mkdir(parents=True)
    executable = bundle / "commonplace"
    executable.write_text(executable_text(version, executable_marker or commit))
    executable.chmod(0o755)
    (bundle / "Cargo.lock").write_text("lock\n")
    (bundle / "models.json").write_text("[]\n")
    (bundle / "USAGE.txt").write_text("use installer\n")
    (bundle / "pinned-models" / "revision" / "model.bin").write_bytes(commit.encode())
    (bundle / "provenance.json").write_text(json.dumps({
        "format": "commonplace-package-provenance/1",
        "package": {"name": "commonplace", "version": version},
        "source_commit": commit,
        "source_tree": "f" * 40,
        "target": "aarch64-apple-darwin",
        "evidence": {"fixture": True},
    }, indent=2) + "\n")
    inventory = sorted(path for path in bundle.rglob("*") if path.is_file())
    checksums = "".join(
        f"{digest(path)}  {path.relative_to(bundle).as_posix()}\n"
        for path in inventory
    )
    if bad_bundle_checksum:
        checksums = "0" * 64 + checksums[64:]
    (bundle / "SHA256SUMS").write_text(checksums)
    if missing_usage:
        (bundle / "USAGE.txt").unlink()
    if extra_file:
        (bundle / "unexpected.txt").write_text("unexpected\n")
    if extra_directory:
        (bundle / "unexpected-empty-directory").mkdir()
    archive = root / f"{release_id}.tar.gz"
    with tarfile.open(archive, "w:gz") as output:
        output.add(bundle, arcname=release_id)
    Path(f"{archive}.sha256").write_text(f"{digest(archive)}  {archive.name}\n")
    shutil.rmtree(bundle)
    return archive, release_id


def make_custom_archive(root, release_id, members):
    archive = root / f"{release_id}.tar.gz"
    with tarfile.open(archive, "w:gz") as output:
        for info, content in members:
            output.addfile(info, io.BytesIO(content) if content is not None else None)
    Path(f"{archive}.sha256").write_text(f"{digest(archive)}  {archive.name}\n")
    return archive


def make_manual_install(home, *, mutate=None):
    version = "0.1.0"
    source_commit = "4ec9039fe77a050917d1745568c69c9163fd098a"
    release_id = f"commonplace-{version}-{source_commit[:12]}-manual-macos-arm64"
    release = home / ".local/share/commonplace/releases" / release_id
    release.mkdir(parents=True)
    release_executable = release / "commonplace"
    release_executable.write_text(executable_text(version, source_commit))
    release_executable.chmod(0o555)
    stable = home / ".local/bin/commonplace"
    stable.parent.mkdir(parents=True)
    shutil.copyfile(release_executable, stable)
    stable.chmod(0o755)
    receipt = {
        "architecture": "arm64",
        "executable_path": str(stable),
        "executable_sha256": digest(stable),
        "format": "commonplace-manual-install/1",
        "installed_at": "2026-09-30T20:39:24Z",
        "main_commit": "ed1c19009314e2b6bdc8f0a376ac7c58c4b1e115",
        "package_version": version,
        "platform": "macos",
        "release_id": release_id,
        "release_path": str(release),
        "source_commit": source_commit,
        "source_tree": "b81716a26be84f414d5fe18cf4edcaf5aad46561",
    }
    if mutate is not None:
        mutate(receipt, release, stable)
    receipt_path = home / ".local/share/commonplace/installed-release.json"
    receipt_path.write_text(json.dumps(receipt, indent=2, sort_keys=True) + "\n")
    return receipt, release, stable, receipt_path


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.root = Path(self.temporary.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.real_receipt = Path.home() / ".local/share/commonplace/installed-release.json"
        self.real_receipt_before = (
            self.real_receipt.read_bytes() if self.real_receipt.is_file() else None
        )
        self.native = mock.patch.object(installer, "verify_native_executable", lambda path: None)
        self.native.start()

    def tearDown(self):
        self.native.stop()
        after = self.real_receipt.read_bytes() if self.real_receipt.is_file() else None
        self.assertEqual(after, self.real_receipt_before)
        self.temporary.cleanup()

    def install(self, archive, **kwargs):
        return self.install_home(self.home, archive, **kwargs)

    def install_home(self, home, archive, **kwargs):
        return installer.install_archive(
            archive,
            home,
            check_platform=False,
            now=kwargs.pop("now", lambda: "2026-09-30T20:00:00Z"),
            **kwargs,
        )

    def receipt(self):
        path = self.home / ".local/share/commonplace/installed-release.json"
        return json.loads(path.read_text())

    def pending_path(self):
        return self.home / ".local/share/commonplace/installed-release.pending.json"

    def test_smoke_rejects_missing_config_command(self):
        executable = self.root / "commonplace"
        executable.write_text(
            executable_text(
                "0.1.0",
                "missing-config",
                tuple(command for command in COMMANDS if command != "config"),
            )
        )
        executable.chmod(0o755)
        with self.assertRaisesRegex(installer.InstallerError, "config"):
            installer.smoke_executable(executable, "0.1.0")

    def test_first_install_idempotent_reinstall_and_verified_update(self):
        first, first_id = make_package(self.root / "first", commit_char="a")
        first.parent.mkdir(exist_ok=True)

        state, executable, receipt_path = self.install(first)
        self.assertEqual(state, "installed")
        first_receipt_bytes = receipt_path.read_bytes()
        first_executable_bytes = executable.read_bytes()
        first_receipt = self.receipt()
        self.assertEqual(first_receipt["release_id"], first_id)
        self.assertEqual(first_receipt["archive_sha256"], digest(first))
        self.assertEqual(first_receipt["executable_sha256"], digest(executable))

        state, _, _ = self.install(
            first,
            now=lambda: "2099-01-01T00:00:00Z",
        )
        self.assertEqual(state, "unchanged")
        self.assertEqual(receipt_path.read_bytes(), first_receipt_bytes)
        self.assertEqual(executable.read_bytes(), first_executable_bytes)

        second_root = self.root / "second"
        second_root.mkdir()
        second, second_id = make_package(
            second_root,
            version="0.2.0",
            commit_char="b",
        )
        state, _, _ = self.install(second, now=lambda: "2026-10-01T01:02:03Z")
        self.assertEqual(state, "updated")
        second_receipt = self.receipt()
        self.assertEqual(second_receipt["release_id"], second_id)
        self.assertEqual(second_receipt["installed_at"], "2026-10-01T01:02:03Z")
        releases = self.home / ".local/share/commonplace/releases"
        self.assertTrue((releases / first_id).is_dir())
        self.assertTrue((releases / second_id).is_dir())

    def test_preserves_unrelated_store_configuration_and_reports(self):
        root = self.home / ".local/share/commonplace"
        unrelated = {
            root / "stores/personal/store.txt": b"store",
            root / "config/settings.json": b"config",
            root / "reports/run.txt": b"report",
        }
        for path, content in unrelated.items():
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(content)
        package_root = self.root / "package"
        package_root.mkdir()
        archive, _ = make_package(package_root)
        self.install(archive)
        for path, content in unrelated.items():
            self.assertEqual(path.read_bytes(), content)

    def test_rejects_missing_bad_and_misnamed_sidecars(self):
        for case in ("missing", "bad-hash", "bad-name", "bad-format"):
            with self.subTest(case=case):
                root = self.root / case
                root.mkdir()
                archive, _ = make_package(root)
                sidecar = Path(f"{archive}.sha256")
                if case == "missing":
                    sidecar.unlink()
                elif case == "bad-hash":
                    sidecar.write_text(f"{'0' * 64}  {archive.name}\n")
                elif case == "bad-name":
                    sidecar.write_text(f"{digest(archive)}  other.tar.gz\n")
                else:
                    sidecar.write_text(f"{digest(archive)} *{archive.name}\n")
                with self.assertRaises(installer.InstallerError):
                    self.install(archive)

    def test_rejects_bad_bundle_checksum_and_unlisted_file(self):
        for case, options in (
            ("bad-checksum", {"bad_bundle_checksum": True}),
            ("extra-file", {"extra_file": True}),
            ("extra-directory", {"extra_directory": True}),
            ("missing-required", {"missing_usage": True}),
        ):
            with self.subTest(case=case):
                root = self.root / case
                root.mkdir()
                archive, _ = make_package(root, **options)
                with self.assertRaises(installer.InstallerError):
                    self.install(archive)

    def test_rejects_unsafe_tar_members(self):
        release_id = f"commonplace-0.1.0-{'a' * 12}-macos-arm64"
        cases = {}

        traversal = tarfile.TarInfo(f"{release_id}/../escape")
        traversal.size = 1
        cases["traversal"] = [(traversal, b"x")]

        absolute = tarfile.TarInfo("/absolute")
        absolute.size = 1
        cases["absolute"] = [(absolute, b"x")]

        link = tarfile.TarInfo(f"{release_id}/link")
        link.type = tarfile.SYMTYPE
        link.linkname = "/tmp"
        cases["link"] = [(link, None)]

        fifo = tarfile.TarInfo(f"{release_id}/fifo")
        fifo.type = tarfile.FIFOTYPE
        cases["special"] = [(fifo, None)]

        directory = tarfile.TarInfo(release_id)
        directory.type = tarfile.DIRTYPE
        duplicate_one = tarfile.TarInfo(f"{release_id}/same")
        duplicate_one.size = 1
        duplicate_two = tarfile.TarInfo(f"{release_id}/same")
        duplicate_two.size = 1
        cases["duplicate"] = [
            (directory, None),
            (duplicate_one, b"a"),
            (duplicate_two, b"b"),
        ]

        for case, members in cases.items():
            with self.subTest(case=case):
                root = self.root / case
                root.mkdir()
                archive = make_custom_archive(root, release_id, members)
                with self.assertRaises(installer.InstallerError):
                    self.install(archive)

    def test_conflicting_release_and_unmanaged_executable_fail_closed(self):
        root = self.root / "package"
        root.mkdir()
        archive, release_id = make_package(root)
        releases = self.home / ".local/share/commonplace/releases"
        conflict = releases / release_id
        conflict.mkdir(parents=True)
        (conflict / "unexpected").write_text("conflict")
        with self.assertRaises(installer.InstallerError):
            self.install(archive)

        shutil.rmtree(self.home / ".local")
        executable = self.home / ".local/bin/commonplace"
        executable.parent.mkdir(parents=True)
        executable.write_bytes(b"unmanaged")
        executable.chmod(0o755)
        with self.assertRaises(installer.InstallerError):
            self.install(archive)

    def test_malformed_receipt_and_missing_executable_fail_closed(self):
        root = self.root / "package"
        root.mkdir()
        archive, _ = make_package(root)
        receipt = self.home / ".local/share/commonplace/installed-release.json"
        receipt.parent.mkdir(parents=True)
        receipt.write_text("{}\n")
        with self.assertRaises(installer.InstallerError):
            self.install(archive)

        shutil.rmtree(self.home / ".local")
        self.install(archive)
        (self.home / ".local/bin/commonplace").unlink()
        with self.assertRaises(installer.InstallerError):
            self.install(archive)

    def test_pre_activation_failure_preserves_prior_executable_and_receipt(self):
        first_root = self.root / "first"
        second_root = self.root / "second"
        first_root.mkdir()
        second_root.mkdir()
        first, _ = make_package(first_root, commit_char="a")
        second, _ = make_package(second_root, version="0.2.0", commit_char="b")
        _, executable, receipt_path = self.install(first)
        executable_before = executable.read_bytes()
        receipt_before = receipt_path.read_bytes()

        def fail_executable(source, destination):
            if destination == executable:
                raise OSError("injected executable publication failure")
            os.replace(source, destination)

        with self.assertRaises(installer.InstallerError) as raised:
            self.install(second, replace=fail_executable)
        self.assertNotIsInstance(raised.exception, installer.PartialInstallError)
        self.assertIn(str(self.pending_path()), str(raised.exception))
        self.assertTrue(self.pending_path().is_file())
        self.assertEqual(executable.read_bytes(), executable_before)
        self.assertEqual(receipt_path.read_bytes(), receipt_before)
        state, _, _ = self.install(second)
        self.assertEqual(state, "updated")
        self.assertFalse(self.pending_path().exists())

    def test_first_install_receipt_failure_retries_same_archive(self):
        root = self.root / "package"
        other_root = self.root / "other"
        root.mkdir()
        other_root.mkdir()
        archive, release_id = make_package(root, commit_char="a")
        other, _ = make_package(other_root, version="0.2.0", commit_char="b")
        receipt_path = self.home / ".local/share/commonplace/installed-release.json"

        def fail_receipt(source, destination):
            if destination == receipt_path:
                raise OSError("injected receipt publication failure")
            os.replace(source, destination)

        with self.assertRaises(installer.PartialInstallError):
            self.install(archive, replace=fail_receipt)
        executable = self.home / ".local/bin/commonplace"
        self.assertTrue(executable.is_file())
        self.assertFalse(receipt_path.exists())
        self.assertTrue(self.pending_path().is_file())
        with self.assertRaises(installer.InstallerError) as raised:
            self.install(other)
        self.assertIn(str(self.pending_path()), str(raised.exception))

        state, _, _ = self.install(archive)
        self.assertEqual(state, "installed")
        self.assertEqual(self.receipt()["release_id"], release_id)
        self.assertFalse(self.pending_path().exists())

    def test_update_receipt_failure_retries_same_archive(self):
        first_root = self.root / "first"
        second_root = self.root / "second"
        third_root = self.root / "third"
        first_root.mkdir()
        second_root.mkdir()
        third_root.mkdir()
        first, _ = make_package(first_root, commit_char="a")
        second, second_id = make_package(second_root, version="0.2.0", commit_char="b")
        third, _ = make_package(third_root, version="0.3.0", commit_char="c")
        self.install(first)
        receipt_path = self.home / ".local/share/commonplace/installed-release.json"
        old_receipt = receipt_path.read_bytes()

        def fail_receipt(source, destination):
            if destination == receipt_path:
                raise OSError("injected receipt publication failure")
            os.replace(source, destination)

        with self.assertRaises(installer.PartialInstallError):
            self.install(second, replace=fail_receipt)
        self.assertEqual(receipt_path.read_bytes(), old_receipt)
        self.assertTrue(self.pending_path().is_file())
        with self.assertRaises(installer.InstallerError) as raised:
            self.install(third)
        self.assertIn(str(self.pending_path()), str(raised.exception))
        state, _, _ = self.install(second)
        self.assertEqual(state, "updated")
        self.assertEqual(self.receipt()["release_id"], second_id)
        self.assertFalse(self.pending_path().exists())

    def test_byte_identical_different_archive_cannot_supersede_partial_update(self):
        first_root = self.root / "first"
        second_root = self.root / "second"
        third_root = self.root / "third"
        first_root.mkdir()
        second_root.mkdir()
        third_root.mkdir()
        first, _ = make_package(first_root, executable_marker="identical")
        second, _ = make_package(
            second_root,
            commit_char="b",
            executable_marker="identical",
        )
        third, third_id = make_package(
            third_root,
            commit_char="c",
            executable_marker="identical",
        )
        self.install(first)
        receipt_path = self.home / ".local/share/commonplace/installed-release.json"
        old_receipt = receipt_path.read_bytes()

        def fail_receipt(source, destination):
            if destination == receipt_path:
                raise OSError("injected receipt publication failure")
            os.replace(source, destination)

        with self.assertRaises(installer.PartialInstallError):
            self.install(second, replace=fail_receipt)
        self.assertEqual(receipt_path.read_bytes(), old_receipt)
        with self.assertRaises(installer.InstallerError) as raised:
            self.install(third)
        self.assertIn(str(self.pending_path()), str(raised.exception))
        state, _, _ = self.install(second)
        self.assertEqual(state, "updated")
        self.assertNotEqual(self.receipt()["release_id"], third_id)
        self.assertFalse(self.pending_path().exists())

    def test_release_publication_interruption_is_pre_activation_and_retryable(self):
        root = self.root / "package"
        root.mkdir()
        archive, release_id = make_package(root)
        real_rename = installer.os.rename

        def fail_release(source, destination):
            if destination.name == release_id:
                raise OSError("injected release publication failure")
            real_rename(source, destination)

        with mock.patch.object(installer.os, "rename", fail_release):
            with self.assertRaises(installer.InstallerError):
                self.install(archive)
        self.assertFalse((self.home / ".local/bin/commonplace").exists())
        self.assertFalse((self.home / ".local/share/commonplace/installed-release.json").exists())
        state, _, _ = self.install(archive)
        self.assertEqual(state, "installed")

    def test_post_activation_smoke_failure_is_partial_and_retryable(self):
        root = self.root / "package"
        root.mkdir()
        archive, _ = make_package(root)
        stable = self.home / ".local/bin/commonplace"
        real_smoke = installer.smoke_executable

        def fail_stable(executable, version):
            if executable == stable:
                raise installer.InstallerError("injected stable smoke failure")
            real_smoke(executable, version)

        with mock.patch.object(installer, "smoke_executable", fail_stable):
            with self.assertRaises(installer.PartialInstallError):
                self.install(archive)
        self.assertTrue(stable.is_file())
        self.assertFalse((self.home / ".local/share/commonplace/installed-release.json").exists())
        state, _, _ = self.install(archive)
        self.assertEqual(state, "installed")

    def test_adopts_exact_manual_install_and_preserves_manual_release(self):
        manual_receipt, manual_release, _, receipt_path = make_manual_install(self.home)
        manual_binary = (manual_release / "commonplace").read_bytes()
        root = self.root / "package"
        root.mkdir()
        archive, canonical_id = make_package(
            root,
            version="0.2.0",
            commit_char="b",
        )

        state, _, _ = self.install(archive)
        self.assertEqual(state, "updated")
        canonical = json.loads(receipt_path.read_text())
        self.assertEqual(canonical["format"], "commonplace-installed-release/1")
        self.assertEqual(canonical["release_id"], canonical_id)
        self.assertNotIn("main_commit", canonical)
        self.assertNotIn("source_tree", canonical)
        self.assertEqual((manual_release / "commonplace").read_bytes(), manual_binary)
        self.assertEqual(manual_receipt["release_path"], str(manual_release))

    def test_manual_pre_activation_failure_preserves_manual_state(self):
        _, manual_release, stable, receipt_path = make_manual_install(self.home)
        stable_before = stable.read_bytes()
        receipt_before = receipt_path.read_bytes()
        root = self.root / "package"
        root.mkdir()
        archive, _ = make_package(root, version="0.2.0", commit_char="b")

        def fail_executable(source, destination):
            if destination == stable:
                raise OSError("injected executable publication failure")
            os.replace(source, destination)

        with self.assertRaises(installer.InstallerError) as raised:
            self.install(archive, replace=fail_executable)
        self.assertNotIsInstance(raised.exception, installer.PartialInstallError)
        self.assertEqual(stable.read_bytes(), stable_before)
        self.assertEqual(receipt_path.read_bytes(), receipt_before)
        self.assertTrue(manual_release.is_dir())

    def test_manual_receipt_failure_retries_different_incoming_version(self):
        _, manual_release, stable, receipt_path = make_manual_install(self.home)
        manual_receipt = receipt_path.read_bytes()
        root = self.root / "package"
        root.mkdir()
        archive, canonical_id = make_package(
            root,
            version="0.2.0",
            commit_char="b",
        )

        def fail_receipt(source, destination):
            if destination == receipt_path:
                raise OSError("injected receipt publication failure")
            os.replace(source, destination)

        with self.assertRaises(installer.PartialInstallError):
            self.install(archive, replace=fail_receipt)
        self.assertEqual(receipt_path.read_bytes(), manual_receipt)
        self.assertTrue(self.pending_path().is_file())
        self.assertIn(b"commonplace 0.2.0", stable.read_bytes())
        state, _, _ = self.install(archive)
        self.assertEqual(state, "updated")
        self.assertEqual(self.receipt()["release_id"], canonical_id)
        self.assertTrue(manual_release.is_dir())
        self.assertFalse(self.pending_path().exists())

    def test_exact_completed_pending_marker_is_cleared_idempotently(self):
        root = self.root / "package"
        root.mkdir()
        archive, _ = make_package(root)
        self.install(archive)
        receipt = self.receipt()
        pending = {
            "format": "commonplace-installed-release-pending/1",
            **{field: receipt[field] for field in (
                "package_version",
                "source_commit",
                "release_id",
                "release_path",
                "executable_path",
                "executable_sha256",
                "archive_sha256",
            )},
        }
        self.pending_path().write_text(json.dumps(pending, indent=2, sort_keys=True) + "\n")
        state, _, _ = self.install(archive)
        self.assertEqual(state, "unchanged")
        self.assertFalse(self.pending_path().exists())

    def test_malformed_or_mismatched_pending_marker_refuses(self):
        root = self.root / "package"
        root.mkdir()
        archive, _ = make_package(root)
        self.install(archive)
        receipt = self.receipt()
        pending = {
            "format": "commonplace-installed-release-pending/1",
            **{field: receipt[field] for field in (
                "package_version",
                "source_commit",
                "release_id",
                "release_path",
                "executable_path",
                "executable_sha256",
                "archive_sha256",
            )},
        }
        for case, mutation in (
            ("unknown", lambda value: value.update({"extra": "x"})),
            ("mismatch", lambda value: value.update({"archive_sha256": "0" * 64})),
        ):
            with self.subTest(case=case):
                self.pending_path().write_text(
                    json.dumps(pending, indent=2, sort_keys=True) + "\n"
                )
                value = json.loads(self.pending_path().read_text())
                mutation(value)
                self.pending_path().write_text(
                    json.dumps(value, indent=2, sort_keys=True) + "\n"
                )
                with self.assertRaises(installer.InstallerError):
                    self.install(archive)

    def test_pending_marker_removal_failure_is_partial_and_retryable(self):
        root = self.root / "package"
        root.mkdir()
        archive, _ = make_package(root)
        real_remove = installer.remove_pending
        calls = 0

        def fail_once(path):
            nonlocal calls
            calls += 1
            if calls == 1:
                raise installer.InstallerError("injected pending removal failure")
            real_remove(path)

        with mock.patch.object(installer, "remove_pending", fail_once):
            with self.assertRaises(installer.PartialInstallError):
                self.install(archive)
        self.assertTrue(self.pending_path().is_file())
        state, _, _ = self.install(archive)
        self.assertEqual(state, "unchanged")
        self.assertFalse(self.pending_path().exists())

    def test_failure_before_pending_marker_leaves_no_marker(self):
        root = self.root / "package"
        root.mkdir()
        archive, release_id = make_package(root)
        real_smoke = installer.smoke_executable

        def fail_release_smoke(executable, version):
            if release_id in str(executable):
                raise installer.InstallerError("injected release smoke failure")
            real_smoke(executable, version)

        with mock.patch.object(installer, "smoke_executable", fail_release_smoke):
            with self.assertRaises(installer.InstallerError):
                self.install(archive)
        self.assertFalse(self.pending_path().exists())

    def test_rejects_malformed_or_mismatched_manual_install(self):
        cases = {
            "unknown-field": lambda receipt, release, stable: receipt.update({"extra": "x"}),
            "wrong-path": lambda receipt, release, stable: receipt.update(
                {"release_path": str(release.parent / "other")}
            ),
            "wrong-stable-hash": lambda receipt, release, stable: stable.write_bytes(b"changed"),
            "wrong-release-hash": lambda receipt, release, stable: (
                (release / "commonplace").chmod(0o755),
                (release / "commonplace").write_bytes(b"changed"),
            ),
            "extra-release-entry": lambda receipt, release, stable: (
                release / "extra"
            ).write_text("extra"),
            "unknown-format": lambda receipt, release, stable: receipt.update(
                {"format": "commonplace-manual-install/2"}
            ),
        }
        package_root = self.root / "package"
        package_root.mkdir()
        archive, _ = make_package(package_root, version="0.2.0", commit_char="b")
        for case, mutate in cases.items():
            with self.subTest(case=case):
                home = self.root / f"home-{case}"
                home.mkdir()
                make_manual_install(home, mutate=mutate)
                with self.assertRaises(installer.InstallerError):
                    self.install_home(home, archive)


if __name__ == "__main__":
    unittest.main()
