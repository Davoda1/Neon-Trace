#!/usr/bin/env python3
"""Regenerate cached vector layers from the project's Simplemaps SVG.

Usage: python tools/prepare_map.py
       python tools/prepare_map.py assets/world-o2.svg --output-root /tmp/map-build
Requires Shapely 2.x for build-time topology only. The application has no Python
or Shapely dependency. Original SVG path strings are preserved verbatim.
A standalone SVG or HTML file containing #geography-wide can be used as input.
"""
from pathlib import Path
import argparse
import html
import re
import struct
from shapely import make_valid
from shapely.geometry import Polygon
from shapely.ops import linemerge, unary_union


def parse_rings(d):
    tokens = re.findall(r"[MLZmlz]|[-+]?(?:[0-9]*\.[0-9]+|[0-9]+)(?:[eE][-+]?[0-9]+)?", d)
    current = (0.0, 0.0)
    start = current
    rings, ring, mode, i = [], [], None, 0
    while i < len(tokens):
        if tokens[i].isalpha():
            mode = tokens[i]
            i += 1
            if mode in "Zz":
                if ring:
                    rings.append(ring)
                    ring = []
                current = start
                continue
        else:
            x, y = float(tokens[i]), float(tokens[i + 1])
            i += 2
            if mode in "ml":
                x, y = x + current[0], y + current[1]
            current = (round(x, 4), round(y, 4))
            if mode in "Mm":
                start = current
            ring.append(current)
            if mode == "M":
                mode = "L"
            elif mode == "m":
                mode = "l"
    if ring:
        rings.append(ring)
    return rings


def lines(geometry):
    if geometry.is_empty:
        return []
    if geometry.geom_type in ("LineString", "LinearRing"):
        return [list(geometry.coords)]
    return [r for g in geometry.geoms for r in lines(g)]


def prepare(source, root):
    content = source.read_text(encoding="utf-8")
    group = re.search(r'<g\b[^>]*\bid=["\']geography-wide["\'][^>]*>', content)
    if group is None:
        raise ValueError("Input must contain the geography-wide SVG group")
    depth = 1
    end = None
    for tag in re.finditer(r'</?g\b[^>]*>', content[group.end():]):
        depth += -1 if tag.group().startswith("</") else 1
        if depth == 0:
            end = group.end() + tag.end()
            break
    if end is None:
        raise ValueError("The geography-wide group is not closed")
    paths = re.findall(r'<path[^>]* d="([^"]+)"[^>]*><title>(.*?)</title></path>', content[group.start():end])
    if len(paths) != 470 or len(set(name for _, name in paths)) != 220:
        raise ValueError("Expected 470 polygon parts and 220 country/territory names")
    notice = next((comment for comment in re.findall(r'<!--.*?-->', content, re.DOTALL)
                   if "Copyright (c) 2020 Pareto Softare, LLC DBA Simplemaps.com" in comment), None)
    if notice is None or "Permission is hereby granted" not in notice or "THE SOFTWARE IS PROVIDED" not in notice:
        raise ValueError("The complete Simplemaps MIT notice is required")
    assets = root / "assets"
    assets.mkdir(parents=True, exist_ok=True)
    svg = '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 2000 857">\n'
    svg += notice + '\n<g id="geography-wide" fill-rule="nonzero">\n'
    for i, (d, name) in enumerate(paths):
        svg += f'<path id="wide-part-{i}" d="{d}"><title>{name}</title></path>\n'
    svg += "</g></svg>\n"
    (assets / "world-o2.svg").write_text(svg, encoding="utf-8")
    (assets / "SIMPLEMAPS-LICENSE.txt").write_text(notice[4:-3].strip() + "\n\nSource: https://simplemaps.com/resources/svg-world\n", encoding="utf-8")
    records = [(html.unescape(name), parse_rings(d)) for d, name in paths]
    shapes = [make_valid(Polygon(rings[0], rings[1:])) for _, rings in records]
    silhouette = unary_union(shapes)
    coast = lines(silhouette.boundary)
    # Union before difference removes double-stroked shared boundaries. Coast
    # belongs to the whole land union, never to each individual country.
    borders = lines(linemerge(unary_union([p.boundary for p in shapes]).difference(silhouette.boundary)))
    data = bytearray(b"NTG1")
    u32 = lambda n: data.extend(struct.pack("<I", n))
    def write_line(points):
        u32(len(points))
        for x, y in points:
            data.extend(struct.pack("<ff", x, y))
    u32(len(records))
    for name, rings in records:
        encoded = name.encode("utf-8")
        u32(len(encoded))
        data.extend(encoded)
        u32(len(rings))
        for ring in rings:
            write_line(ring)
    for collection in (coast, borders):
        u32(len(collection))
        for points in collection:
            write_line(points)
    (assets / "world-geometry.bin").write_bytes(data)
    print(f"Preserved {len(paths)} SVG parts / {len(set(n for n, _ in records))} countries; cached {len(coast)} coast rings and {len(borders)} unique internal border lines.")


if __name__ == "__main__":
    project_root = Path(__file__).resolve().parents[1]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", nargs="?", type=Path, default=project_root / "assets" / "world-o2.svg")
    parser.add_argument("--output-root", type=Path, default=project_root,
                        help="Output project directory; generated files are placed in its assets directory")
    arguments = parser.parse_args()
    prepare(arguments.source, arguments.output_root)
