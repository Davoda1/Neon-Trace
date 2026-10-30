#!/usr/bin/env python3
"""Build and verify NeonTrace's eight-file private release set.

This is developer tooling. It is never included in a binary package, and never
creates a launcher or publishes a release. Run after both native builds and
tools/dependency_notices.py have completed.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import stat
import subprocess
import tarfile
import tempfile
import zipfile

ROOT = Path(__file__).resolve().parents[1]
SOURCE_DIRS = {"src", "assets", "tools", "docs", ".cargo", ".github"}
SOURCE_FILES = {"Cargo.toml", "Cargo.lock", "build.rs", "LICENSE", "README.md", ".gitignore", "rust-toolchain.toml"}
FORBIDDEN = re.compile(rb"(?:github_pat_[A-Za-z0-9_]{20,}|ghp_[A-Za-z0-9]{20,})")


def require_binary(path, magic):
    if not path.is_file() or path.is_symlink():
        raise ValueError(f"Missing regular native executable: {path.name}")
    with path.open("rb") as stream:
        if stream.read(len(magic)) != magic:
            raise ValueError(f"Invalid native executable format: {path.name}")
        stream.seek(0)
        header = stream.read(64)
        if magic == b"\x7fELF":
            if len(header) < 20 or header[4:6] != b"\x02\x01" or int.from_bytes(header[18:20], "little") != 62:
                raise ValueError(f"Expected Linux amd64 executable: {path.name}")
        elif magic == b"MZ":
            if len(header) < 64:
                raise ValueError(f"Truncated Windows executable: {path.name}")
            stream.seek(int.from_bytes(header[60:64], "little"))
            pe = stream.read(6)
            if pe[:4] != b"PE\0\0" or int.from_bytes(pe[4:6], "little") != 0x8664:
                raise ValueError(f"Expected Windows x64 executable: {path.name}")


def scan_private(path):
    if "CONFIDENTIAL" in path.name.upper():
        raise ValueError(f"Confidential filename excluded: {path.name}")
    with path.open("rb") as stream:
        overlap = b""
        while block := stream.read(1024 * 1024):
            data = overlap + block
            if FORBIDDEN.search(data):
                raise ValueError(f"Exposed credential rejected: {path.name}")
            overlap = data[-200:]


def source_files():
    result = []
    for path in sorted(ROOT.rglob("*")):
        rel = path.relative_to(ROOT)
        if not path.is_file() or path.is_symlink():
            continue
        if len(rel.parts) == 1 and rel.name not in SOURCE_FILES:
            continue
        if len(rel.parts) > 1 and rel.parts[0] not in SOURCE_DIRS:
            continue
        if any(x in {"__pycache__", ".pytest_cache", "target", ".git"} for x in rel.parts):
            continue
        if path.suffix in {".pyc", ".tmp", ".log"}:
            continue
        scan_private(path)
        result.append(path)
    for required in ["Cargo.toml", "Cargo.lock", "LICENSE", "src/main.rs"]:
        if ROOT / required not in result:
            raise ValueError(f"Incomplete source tree: {required}")
    return result


def create_zip(destination, entries):
    with zipfile.ZipFile(destination, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as archive:
        for local, name in entries:
            archive.write(local, name)


def create_tar(destination, entries, executable_names=()):
    with tarfile.open(destination, "w:gz", compresslevel=9) as archive:
        for local, name in entries:
            info = archive.gettarinfo(local, arcname=name)
            info.uid = info.gid = 0
            info.uname = info.gname = "root"
            if name in executable_names:
                info.mode = 0o755
            with local.open("rb") as source:
                archive.addfile(info, source)


def verify_binary_archives(windows_zip, linux_tar):
    with zipfile.ZipFile(windows_zip) as archive:
        names = archive.namelist()
        assert "NeonTrace.exe" in names and archive.read("NeonTrace.exe")[:2] == b"MZ"
        assert all(not x.lower().endswith((".sh", ".cmd", ".ps1")) for x in names)
    with tarfile.open(linux_tar) as archive:
        binary = archive.getmember("neontrace")
        assert binary.isfile() and not binary.issym() and binary.mode & 0o111
        assert archive.extractfile(binary).read(4) == b"\x7fELF"
        assert all(not x.name.lower().endswith((".sh", ".cmd", ".ps1")) for x in archive.getmembers())


def linux_glibc_floor(binary):
    result = subprocess.run(["readelf", "--version-info", str(binary)], check=True, capture_output=True, text=True)
    versions = [tuple(map(int, m)) for m in re.findall(r"GLIBC_(\d+)\.(\d+)", result.stdout)]
    floor = max(versions, default=(2, 39))
    if floor > (2, 39):
        raise ValueError(f"Linux executable requires glibc {floor}; Ubuntu 24.04 baseline is 2.39")
    return ".".join(map(str, floor))


def build_deb(destination, binary, version, notices, notes):
    floor = linux_glibc_floor(binary)
    with tempfile.TemporaryDirectory(prefix="neontrace-deb-") as temporary:
        stage = Path(temporary)
        for directory in ["DEBIAN", "usr/bin", "usr/share/applications", "usr/share/icons/hicolor/256x256/apps", "usr/share/doc/neontrace"]:
            (stage / directory).mkdir(parents=True, exist_ok=True)
        target = stage / "usr/bin/neontrace"
        shutil.copy2(binary, target)
        target.chmod(0o755)
        shutil.copy2(ROOT / "assets/neontrace.png", stage / "usr/share/icons/hicolor/256x256/apps/neontrace.png")
        shutil.copy2(ROOT / "LICENSE", stage / "usr/share/doc/neontrace/copyright")
        shutil.copy2(notices, stage / "usr/share/doc/neontrace/THIRD-PARTY-NOTICES.txt")
        shutil.copy2(notes, stage / "usr/share/doc/neontrace/README-Notes.txt")
        (stage / "usr/share/applications/neontrace.desktop").write_text("[Desktop Entry]\nType=Application\nName=NeonTrace\nComment=Visual ping and network route explorer\nExec=/usr/bin/neontrace\nIcon=neontrace\nTerminal=false\nCategories=Network;Utility;\nStartupWMClass=neontrace\n", encoding="utf-8")
        (stage / "DEBIAN/control").write_text(
            f"Package: neontrace\nVersion: {version}\nSection: net\nPriority: optional\nArchitecture: amd64\n"
            f"Maintainer: davoda1\nInstalled-Size: {(sum(p.stat().st_size for p in stage.rglob('*') if p.is_file()) + 1023)//1024}\n"
            # winit/softbuffer load display libraries at runtime, so ldd alone
            # does not reveal them. rfd's selected Linux backend uses the
            # desktop portal FileChooser interface; GTK/KDE provide it.
            f"Depends: libc6 (>= {floor}), libgcc-s1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libwayland-cursor0, libx11-6, libx11-xcb1, libxcursor1, libxi6, xdg-desktop-portal, xdg-desktop-portal-gtk | xdg-desktop-portal-kde\n"
            "Recommends: libvulkan1, mesa-vulkan-drivers, libegl1, libgles2, libwayland-egl1\nHomepage: https://github.com/Davoda1/Neon-Trace\n"
            "Description: NeonTrace native visual network route explorer\n"
            " Observed ICMP probes and approximate locations on a fixed world map.\n"
            " Supports an offline demonstration, local IP data, and explicit exports.\n", encoding="utf-8")
        subprocess.run(["dpkg-deb", "--root-owner-group", "--build", str(stage), str(destination)], check=True)
        with tempfile.TemporaryDirectory(prefix="neontrace-deb-check-") as inspection:
            subprocess.run(["dpkg-deb", "--extract", str(destination), inspection], check=True)
            entries = list((Path(inspection) / "usr/bin").iterdir())
            assert len(entries) == 1 and entries[0].name == "neontrace"
            assert entries[0].is_file() and not entries[0].is_symlink()
            require_binary(entries[0], b"\x7fELF")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--linux", type=Path, required=True)
    parser.add_argument("--windows", type=Path, required=True)
    parser.add_argument("--windows-dll-dir", type=Path)
    parser.add_argument("--preview", type=Path, default=ROOT / "docs/NeonTrace-Preview.html")
    parser.add_argument("--notes", type=Path, default=ROOT / "docs/README-Notes.txt")
    parser.add_argument("--notices", type=Path, default=ROOT / "docs/THIRD-PARTY-NOTICES.txt")
    parser.add_argument("--output", type=Path, default=ROOT.parent / "release")
    parser.add_argument("--version", default="1.0.0")
    args = parser.parse_args()
    if not re.fullmatch(r"\d+\.\d+\.\d+(?:-\d+)?", args.version):
        parser.error("Version must be a concrete release version")
    require_binary(args.linux, b"\x7fELF")
    require_binary(args.windows, b"MZ")
    for path in [args.linux, args.windows, args.preview, args.notes, args.notices, ROOT / "LICENSE"]:
        if not path.is_file():
            raise ValueError(f"Required release input missing: {path.name}")
        scan_private(path)
    args.output.mkdir(parents=True, exist_ok=True)
    prefix = f"NeonTrace-{args.version}"
    common = [(ROOT / "LICENSE", "LICENSE"), (args.notes, "README-Notes.txt"), (args.notices, "THIRD-PARTY-NOTICES.txt")]
    windows_entries = [(args.windows, "NeonTrace.exe"), *common]
    if args.windows_dll_dir:
        for path in sorted(args.windows_dll_dir.glob("*.dll")):
            scan_private(path)
            require_binary(path, b"MZ")
            windows_entries.append((path, path.name))
    win = args.output / f"{prefix}-Windows-x64.zip"
    linux = args.output / f"{prefix}-Linux-amd64.tar.gz"
    deb = args.output / f"{prefix}-Linux-amd64.deb"
    create_zip(win, windows_entries)
    create_tar(linux, [(args.linux, "neontrace"), *common], executable_names={"neontrace"})
    build_deb(deb, args.linux, args.version, args.notices, args.notes)
    sources = [(p, f"{prefix}-Source/{p.relative_to(ROOT).as_posix()}") for p in source_files()]
    src_zip = args.output / f"{prefix}-Source.zip"
    src_tar = args.output / f"{prefix}-Source.tar.gz"
    create_zip(src_zip, sources)
    create_tar(src_tar, sources)
    preview = args.output / f"{prefix}-Preview.html"
    notes = args.output / f"{prefix}-README-Notes.txt"
    shutil.copy2(args.preview, preview)
    shutil.copy2(args.notes, notes)
    verify_binary_archives(win, linux)
    outputs = [win, linux, deb, src_zip, src_tar, preview, notes]
    sums = args.output / "SHA256SUM.txt"
    sums.write_text("".join(f"{hashlib.file_digest(p.open('rb'), 'sha256').hexdigest()}  {p.name}\n" for p in outputs), encoding="utf-8")
    print(json.dumps({"files": [str(p.resolve()) for p in outputs + [sums]], "archive_layouts_verified": True}, indent=2))


if __name__ == "__main__":
    main()
