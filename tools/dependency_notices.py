#!/usr/bin/env python3
"""Collect source license/notice texts for both supported target dependency graphs.

Run after cargo fetch --locked, including both target platforms. No network
access is made by this script. It fails on missing license text so omissions
can be resolved before packaging.
"""
import argparse
import json
from pathlib import Path
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parents[1]


def license_files(directory, explicit):
    candidates = set()
    if explicit:
        path = directory / explicit
        if path.is_file():
            candidates.add(path)
    for path in directory.iterdir():
        if path.is_file() and path.name.upper().startswith(("LICENSE", "LICENCE", "COPYING", "NOTICE", "COPYRIGHT", "UNLICENSE")):
            candidates.add(path)
    for sub in ["licenses", "licences", "LICENSES"]:
        if (directory / sub).is_dir():
            candidates.update(p for p in (directory / sub).rglob("*") if p.is_file())
    return sorted(candidates)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=ROOT / "docs/THIRD-PARTY-NOTICES.txt")
    parser.add_argument("--map-license", type=Path, default=ROOT / "assets/SIMPLEMAPS-LICENSE.txt")
    parser.add_argument("--cargo", default="cargo")
    args = parser.parse_args()
    packages_by_id = {}
    for platform in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"]:
        result = subprocess.run([args.cargo, "metadata", "--locked", "--offline", "--format-version", "1", "--filter-platform", platform], cwd=ROOT, check=True, capture_output=True, text=True)
        metadata = json.loads(result.stdout)
        packages_by_id.update((p["id"], p) for p in metadata["packages"])
    version = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["package"]["version"]
    sections = [f"NEONTRACE {version} — THIRD-PARTY NOTICES\n\n" + (ROOT / "LICENSE").read_text(encoding="utf-8")]
    if not args.map_license.is_file():
        raise ValueError("Original Simplemaps MIT notice must exist before packaging")
    sections.append("SELECTED WORLD MAP\nSource: https://simplemaps.com/resources/svg-world\nRobinson world-o2.svg; original geometry retained.\n\n" + args.map_license.read_text(encoding="utf-8"))
    sections.append("IP GEOLOCATION DATA\nIP Geolocation by DB-IP\nhttps://db-ip.com/\nhttps://db-ip.com/db/lite.php\nDB-IP Lite City and ASN data are licensed under Creative Commons Attribution 4.0 International (CC BY 4.0).\nLicense: https://creativecommons.org/licenses/by/4.0/\nLegal code: https://creativecommons.org/licenses/by/4.0/legalcode.en\nData is downloaded separately on first live use, and is not bundled in the application. Dataset edition and provider metadata are recorded with cached data and relevant exports. Data has reduced coverage and accuracy; geographic positions are estimates.\n")
    for notice in sorted((ROOT / "assets").glob("*-NOTICE.txt")):
        sections.append("BUNDLED ASSET NOTICE\n" + notice.name + "\n\n" + notice.read_text(encoding="utf-8"))
    missing = []
    packages = sorted((p for p in packages_by_id.values() if p.get("source")), key=lambda p:(p["name"],p["version"]))
    for package in packages:
        directory = Path(package["manifest_path"]).parent
        header = f"{package['name']} {package['version']}\nSPDX: {package.get('license') or 'see source notice'}\nSource: {package.get('repository') or 'https://crates.io/crates/' + package['name']}\n"
        files = license_files(directory, package.get("license_file"))
        if not files:
            supplemental = ROOT / "assets/dependency-licenses" / f"{package['name']}-{package['version']}.txt"
            if supplemental.is_file():
                sections.append(header + "\nUpstream source notice (exact crate source revision):\n\n" + supplemental.read_text(encoding="utf-8"))
                continue
            missing.append(package["name"] + " " + package["version"])
        body = []
        for path in files:
            try:
                content = path.read_text(encoding="utf-8")
            except UnicodeDecodeError:
                continue
            body.append(f"\n{path.relative_to(directory).as_posix()}\n\n{content}")
        if not body and files:
            missing.append(package["name"] + " " + package["version"] + " (nontext notices)")
        sections.append(header + "".join(body))
    if missing:
        raise ValueError("Missing source license text; resolve before release: " + ", ".join(missing))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text("\n\n" + ("\n\n" + "="*76 + "\n\n").join(sections), encoding="utf-8")
    print(f"Collected exact notices for {len(packages)} locked dependencies: {args.output.name}")


if __name__ == "__main__":
    main()
