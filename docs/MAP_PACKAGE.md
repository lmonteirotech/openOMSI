# Map packages: a pre-built surface under the roads

A map's author (or anyone) can ship a **pre-built** picture of the map next to the map itself. openOMSI
does not compute, convert or triangulate anything: it opens the package, picks the chunks near the
camera and draws them under the roads of the navigator and the city map. All the work (where the data
comes from, georeferencing, triangulation) is done once, beforehand, by the tool of whoever makes the
package.

Why a package and not "openOMSI reads OpenStreetMap or GeoJSON": loading costs a file read, never a
geometry pipeline; the game never needs to know where the data came from; any author (a real city, a
fictional map, any tool) produces the same files.

It follows the precedent of `radio.cfg`: optional files in the map's folder, read only by openOMSI and
ignored by Omsi.exe, so a map made for both games loses nothing. **A map without the folder looks exactly
as before.**

## 1. Location and files

```
<content folder or OMSI 2>/Maps/<Map>/
  global.cfg
  openomsi-realmap/            the package (fixed name, lower case)
    manifest.cfg               required: its presence makes the package count
    surface.oms                the pre-built surface (section 4)
```

- The folder name and the file names are fixed; nothing is searched or configured. A missing file is a
  feature that is off, never an error. No `manifest.cfg`: the package is ignored.
- The folder may lie in the map's own folder or in the same place of another content root: a package for
  a map one does not own goes in the game's content folder (`Maps/<Map>/openomsi-realmap/`), as a mod does,
  so the OMSI 2 installation is never written to. The root with the most priority wins.
- Other files an author may put in the folder (icons, regions, lines) are not read by this version and
  are ignored.

## 2. Manifest

`key = value` lines, `#` comments, UTF-8.

```
format   = 1
grid     = 300
name     = My Map
credit   = (c) OpenStreetMap contributors (ODbL)
# optional colours (any key left out takes the game's own colour for that look)
water    = #7fb2e5
light_water = #9cc3d6
```

- `format`: integer. A reader that does not know it ignores the package.
- `grid`: the author's tile size in metres (section 3). Required, greater than 1.
- `name`: for people. `credit`: shown on the city map; give it when the data needs attribution.
- Colours: the game has a **dark** and a **light** look. `water = #rrggbb` colours the dark map and its
  `light_` twin the light one. Keys: `land water river green grass forest sport farm built rail rail_edge
  border`. The layers are semantic, so the game keeps the last word on how they look.

## 3. Coordinates

Positions are **metres of the author's tile grid**, the numbers OMSI 2 itself shows for the map's
objects: `x = tile_column * grid + position_in_tile`, `y = tile_row * grid + position_in_tile`, x east,
y north. openOMSI turns each position into its own game metres with the grid it uses for the map
(`omsi_map::tile_local_to_world`), which is why the manifest states `grid`: a `[worldcoordinates]` map's
grid is not 300 m. No latitude or longitude is stored, so a map may be bent, shortened or fictional. The
file stores positions as `f32` offsets from an `origin` (an `f64` pair) to keep precision on large maps.

## 4. `surface.oms`

```
magic    "OMSF"
u32      version (= 1)
f64 x2   origin_x, origin_y         (author's grid metres)
f32 x4   min_x, max_x, min_y, max_y (relative to origin)
f32      chunk_size                 (metres, e.g. 2000)
u32      n_strings ; n_strings x { u16 len, utf-8 bytes }
u32      n_chunks
n_chunks x { i32 cx, i32 cy, u64 offset, u32 byte_len }       the index
... the chunks ...
```

Chunk `(cx, cy)` covers `[cx*chunk_size, (cx+1)*chunk_size)` on each axis, relative to `origin`. A reader
finds the chunks near the camera by the index and reads only those. A chunk needs no other chunk.

```
u32  n_fills   ; n_fills x  { u8 class, u32 n_tri, f32[6*n_tri] }         triangles, ready to draw
u32  n_lines   ; n_lines x  { u8 class, u8 flags, f32 width_m, u32 name,
                              u32 n_pts, f32[2*n_pts] }                    polylines
u32  n_labels  ; n_labels x { u8 kind, u8 rank, u32 name, f32 x, f32 y }   names (not read by this version)
```

- Fill `class`: `0 water, 1 green (parks, gardens), 2 built, 3 grass (grass, meadow, scrub), 4 forest (wood,
  forest), 5 sport (pitches, stadiums), 6 farm`.
- Line `class`: `0 river, 1 rail, 2 border`. A river's `width_m` of 5 or more is a river, less a stream. Other
  classes are skipped, so a later version can add more without breaking this one.
- Triangles are already triangulated; there are no holes to resolve. `name` indexes the string table
  (`u32::MAX`: none).
- Everything that is wrong ends in "no package": a short file, a wrong magic or version, a chunk outside
  the file. The reader checks every length before reading and never panics on a bad file.

## 5. What openOMSI does with it

At the map's load, in the background step that reads the road network, openOMSI parses the manifest and
the index of `surface.oms`. The small map (Shift+N) builds the chunks within its radius around the bus;
the city map (Shift+M) builds all of them once. Both draw the surface **under the roads**, on the map's own
ground colour, in the dark or light look, and the city map shows the credit. Nothing is computed from any
other source and nothing is fetched from the network.

Settings (`~/.openomsi/settings.cfg`, the launcher's settings page and the pause menu): `nav_surface`
(on by default) draws the surface; `nav_light` (off) gives the maps a light look.

## 6. Trying it

- **The example in this repository**, `docs/examples/openomsi-realmap/`: a few made-up shapes (a lake, a
  river, a railway, a border, a park, a wood) in the tile grid of the tutorial map **Grundorf**, under 1 KB,
  with no data of OMSI 2 or OpenStreetMap. Copy the folder to `Maps/Grundorf/openomsi-realmap/` in the
  content folder, start Grundorf and open the navigator (Shift+N) and the city map (Shift+M).
- **Real packages and the generator** that makes them are not in this repository (their data is
  OpenStreetMap's, under the ODbL, a different licence from this project's). They live in
  [lmonteirotech/openomsi-map-packages](https://github.com/lmonteirotech/openomsi-map-packages): its
  releases hold test packages (Berlin-Spandau and Projekt Szczecin) and its README explains how to make a
  package for any map.
