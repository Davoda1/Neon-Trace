"""Regression checks for complete source and native package layouts."""
import importlib.util
import hashlib
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest
import zipfile

SPEC = importlib.util.spec_from_file_location("neontrace_package", Path(__file__).with_name("package.py"))
package = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(package)


class PackageTests(unittest.TestCase):
    def source_tree(self, root):
        for name in package.REQUIRED_SOURCE_FILES | {"CHANGELOG.md", "src/window_state/x11.rs", "assets/example.txt"}:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(name + "\n")
        for name in ["target/result", "tools/__pycache__/test.pyc", "docs/transient.log", "unrelated.txt"]:
            path = root / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text("excluded\n")

    def test_source_inventory_includes_build_support_and_tests(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.source_tree(root)
            names = {p.relative_to(root).as_posix() for p in package.source_files(root)}
            self.assertEqual(names, package.REQUIRED_SOURCE_FILES | {"CHANGELOG.md", "src/window_state/x11.rs", "assets/example.txt"})
            (root / "build_support/windows_resources.rs").unlink()
            with self.assertRaisesRegex(ValueError, "Incomplete source tree"):
                package.source_files(root)

    def test_source_archives_round_trip_and_reject_missing_files(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            self.source_tree(root)
            entries = [(p, "NeonTrace-Source/" + p.relative_to(root).as_posix()) for p in package.source_files(root)]
            archive_zip, archive_tar = root / "source.zip", root / "source.tar.gz"
            package.create_zip(archive_zip, entries)
            package.create_tar(archive_tar, entries)
            package.verify_source_archives(archive_zip, archive_tar, entries)
            package.create_zip(archive_zip, entries[:-1])
            with self.assertRaisesRegex(ValueError, "Source ZIP contents"):
                package.verify_source_archives(archive_zip, archive_tar, entries)

    def test_distribution_scan_rejects_credentials(self):
        with tempfile.TemporaryDirectory() as temporary:
            path = Path(temporary) / "example.txt"
            path.write_text("github_" + "pat_" + "a" * 40)
            with self.assertRaisesRegex(ValueError, "credential"):
                package.scan_distribution(path)

    def test_portable_archives_have_regular_root_executables(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pe = bytearray(160)
            pe[:2] = b"MZ"
            pe[60:64] = (128).to_bytes(4, "little")
            pe[128:134] = b"PE\0\0\x64\x86"
            windows = root / "NeonTrace.exe"
            windows.write_bytes(pe)
            elf = bytearray(64)
            elf[:6] = b"\x7fELF\x02\x01"
            elf[18:20] = (62).to_bytes(2, "little")
            linux = root / "neontrace"
            linux.write_bytes(elf)
            package.require_binary(windows, b"MZ")
            package.require_binary(linux, b"\x7fELF")
            archive_zip, archive_tar = root / "windows.zip", root / "linux.tar.gz"
            package.create_zip(archive_zip, [(windows, "NeonTrace.exe")])
            package.create_tar(archive_tar, [(linux, "neontrace")], executable_names={"neontrace"})
            package.verify_binary_archives(archive_zip, archive_tar)
            with tarfile.open(archive_tar) as archive:
                binary = archive.getmember("neontrace")
                self.assertEqual(binary.mode, 0o755)
                self.assertEqual((binary.uid, binary.gid), (0, 0))
            with zipfile.ZipFile(archive_zip) as archive:
                self.assertEqual(archive.namelist(), ["NeonTrace.exe"])

    @unittest.skipUnless(shutil.which("dpkg-deb") and shutil.which("readelf") and Path("/usr/bin/true").is_file(), "Debian packaging tools required")
    def test_debian_package_installs_exactly_one_regular_binary(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            notes, notices = root / "notes.txt", root / "notices.txt"
            notes.write_text("Package layout test\n")
            notices.write_text("Package layout test\n")
            binary = Path("/usr/bin/true")
            deb = root / "neontrace.deb"
            package.build_deb(deb, binary, "1.1.0", notices, notes)
            extracted = root / "installed"
            subprocess.run(["dpkg-deb", "--extract", str(deb), str(extracted)], check=True)
            entries = list((extracted / "usr/bin").iterdir())
            self.assertEqual([p.name for p in entries], ["neontrace"])
            self.assertFalse(entries[0].is_symlink())
            self.assertEqual(entries[0].read_bytes(), binary.read_bytes())

    @unittest.skipUnless(shutil.which("dpkg-deb") and shutil.which("readelf") and Path("/usr/bin/true").is_file(), "Debian packaging tools required")
    def test_complete_release_has_eight_assets_and_matching_checksums(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            pe = bytearray(160)
            pe[:2] = b"MZ"
            pe[60:64] = (128).to_bytes(4, "little")
            pe[128:134] = b"PE\0\0\x64\x86"
            windows = root / "fixture.exe"
            windows.write_bytes(pe)
            output = root / "release"
            subprocess.run([
                sys.executable, str(Path(__file__).with_name("package.py")),
                "--linux", "/usr/bin/true", "--windows", str(windows),
                "--version", "1.1.0", "--output", str(output),
            ], check=True, capture_output=True, text=True)
            self.assertEqual(len(list(output.iterdir())), 8)
            manifest = (output / "SHA256SUM.txt").read_text().splitlines()
            self.assertEqual(len(manifest), 7)
            for line in manifest:
                digest, name = line.split("  ", 1)
                self.assertEqual(hashlib.sha256((output / name).read_bytes()).hexdigest(), digest)
            with tarfile.open(output / "NeonTrace-1.1.0-Source.tar.gz") as archive:
                names = set(archive.getnames())
                for required in package.REQUIRED_SOURCE_FILES:
                    self.assertIn("NeonTrace-1.1.0-Source/" + required, names)


if __name__ == "__main__":
    unittest.main()
