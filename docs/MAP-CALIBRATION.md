# Selected atlas calibration

The sole geography is the Simplemaps wide Robinson map in
`assets/world-o2.svg`. It contains all 470 original `d` strings, all 220
country/territory names, the 471 rings including the South Africa hole,
and the original full MIT notice. The authoritative source coordinates are
2000 × 857, with all path geometry retained in that coordinate space.

The asset-specific projection is Robinson with linear interpolation between the
standard five-degree coefficient tables, followed by this calibration:

```
x = 983.6 + 331.57069 × longitude_radians × X(abs(latitude))
y = 501.35 − 515.98 × sign(latitude) × Y(abs(latitude))
```

Projection regression fixtures in `src/map.rs` cover eleven city locations and
agree with their expected asset coordinates within 0.001 original SVG units: Budapest, Frankfurt, New York, Toronto, Kyoto, São Paulo, London, Paris,
Amsterdam, Stockholm, and Alice Springs. Independent source-polygon containment
checks additionally cover Beijing, Johannesburg, Buenos Aires, and Moscow.
These checks establish consistent broad geographical placement, not survey
accuracy or a guarantee about IP geolocation. Near small islands and generalized
coasts, accurate city coordinates can fall just outside a simplified polygon;
for example Wellington is about 4.33 source units offshore in this asset and
Reykjavík about 0.91. Coordinates are never snapped to land.

The renderer uses one uniform geographic scale. With no located route, it fits
the complete original 2000 × 857 map inside the available area with glow padding.
Located routes receive automatic framing with context padding and a four-times
world-fit zoom cap. Ctrl + scroll provides pointer-centered manual zoom from
one to twelve times world fit; Ctrl + drag pans. Horizontal copies wrap around
the atlas, while vertical movement is bounded. Fit route restores the automatic
camera. Marker hit testing uses the same transformation as drawing.

Unsupported latitude values
below −60° or above +84°, invalid coordinates, and projected positions outside
the actual asset rectangle remain explicitly unlocated. Antarctica is absent.
Dateline-crossing schematic connections split at ±180° and the two portions
are clipped to the map rectangle. No edge-to-edge arc is drawn through the
middle of the world. Clipping affects route drawing, never estimated positions.

`tools/prepare_map.py` derives static coastlines as the boundary of the union
of the original polygons. Internal country boundaries are unioned once and the
coastline subtracted. This produces 321 coastline rings and 330 internal border
line strips, avoiding bright or double-stroked country borders. Original land
geometry remains separately preserved and is the authoritative filled layer.
The compact binary cache is checked in; Python/Shapely is needed only to
regenerate it, not to run or build the application.

To regenerate the vector cache from the existing project SVG, install Shapely
2.x in a Python environment and run this command from the repository root:

```console
python tools/prepare_map.py
```

The generator reads `assets/world-o2.svg` by default. An optional source path
accepts another copy of that same SVG or an HTML document containing its
`geography-wide` group. Use `--output-root` to write generated assets into a
separate directory for comparison. The generator checks the part/name counts
and requires the complete Simplemaps MIT notice before writing any output.

Source and full notice: https://simplemaps.com/resources/svg-world and
`assets/SIMPLEMAPS-LICENSE.txt`.
