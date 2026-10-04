# Example: a package for Berlin-Spandau, made from OpenStreetMap

A package for the stock map **Berlin-Spandau**, which every OMSI 2 owner has, so that a map package can be tried
without any other map. It is made entirely from OpenStreetMap by [osm_to_package.py](osm_to_package.py), which is
the documentation of how: it is short, has no dependency but Python 3, and works for any map with a
`[worldcoordinates]` section in its `global.cfg` (a map of a real place).

What it adds to the navigator and the city map (see [MAP_PACKAGE.md](../../MAP_PACKAGE.md) for the format):

| File | Content (1.75 MB in all) |
|---|---|
| `surface.oms` | water, green areas, woods, sports grounds, built-up land, rivers, rail and district borders in 69 chunks (46,000 triangles); 12,000 named street pieces (only looked up, to say which street you are on); place names |
| `regions.cfg` | the districts (Ortsteile, `admin_level=10`): outline and the box each name fits in |
| `markers.cfg` | the 17 railway stations of the map, with `icons/terminal.png` |
| `lines.lin` | the 28 bus lines that run in Spandau, with their routes |
| `manifest.cfg` | format 1, the tile size of the map (371.934 m), the credit |

## Try it

1. Copy the `openomsi-realmap` folder to **`Maps/Berlin-Spandau/openomsi-realmap/` in openOMSI's own content
   folder** (the folder next to `openomsi.exe`, where mods go; the OMSI 2 installation is never written to). Putting it
   in the installation's own `maps/Berlin-Spandau/` works as well.
2. Start Berlin-Spandau with any bus and press **Shift+M**. You should see: the surface under the roads; a chip with
   the district and the street ("Falkenhagener Feld", "You are on: ...") and the same in a row under the next stop on the
   small navigator; the districts' names as a faint watermark when zoomed out; the stations; a **Lines** button with
   the 28 bus lines, whose ways and stops are drawn when ticked and whose vehicles take the line's colour.
3. `nav_surface=0` in the settings (or the in-game menu) turns it all off; `nav_light=1` is the light look.

Without a window, as the screenshots in the pull request were made (the city map opened by `OMSI_NAV_MAP=1`, zoom
by `OMSI_NAV_MAP_MPP`, metres per pixel):

```
OMSI_NAV_MAP=1 OMSI_NAV_MAP_MPP=3 openomsi --root "<OMSI 2>" --map maps/Berlin-Spandau/global.cfg \
  --bus Vehicles/MAN_SD202/MAN_D86.bus --time 14:00 --entry 0 --traffic 55 --offscreen out.png --size 1600x900 --drive 6
```

## How it was made

```
python osm_to_package.py --map-dir "<OMSI 2>/maps/Berlin-Spandau" --out openomsi-realmap --cache osm-cache \
    --icons <a folder with terminal.png> --name Berlin-Spandau --area-name Spandau
```

1. **The map's grid.** `global.cfg` lists the tiles (`[map]`: column, row, file) and, with `[worldcoordinates]`, says
   they are web-mercator zoom-16 tiles: column + 32768 is the tile's column on the globe, the row counts north from the
   equator. So a latitude and longitude map to a tile and a place in it with no fitting, and the tile size openOMSI lays
   the map out with is the median row's width (371.934 m for Spandau, the same formula as `omsi_map::world_tile_size`).
   Positions go into the package as `tile * grid + place in the tile`, with `grid` in the manifest.
2. **Download.** The map's tiles are grouped in blocks of 8 x 8 tiles; for each block that has a tile, one request
   to the Overpass API asks for water, wetland, woods, parks, sports grounds, land use, waterways, railways, named
   drivable streets, place nodes and railway stations. Two more requests ask for the districts and for the bus routes
   of the borough. Every answer is kept in `--cache`, the script waits between requests, and a second run asks for
   nothing. (Spandau: 21 blocks and 2 requests, a few minutes.)
3. **Areas.** Closed ways and the outer rings of `natural=water` relations become polygons, simplified to 3 m and cut
   into triangles by ear clipping (holes are not cut, so an island in a lake is covered by the lake). Classes:

   | OpenStreetMap | class |
   |---|---|
   | `natural=water\|wetland`, `landuse=reservoir\|basin` | water |
   | `leisure=park\|garden\|recreation_ground`, `landuse=cemetery` | green |
   | `landuse=grass\|meadow\|allotments`, `natural=scrub\|grassland\|heath` | grass |
   | `natural=wood`, `landuse=forest` | forest |
   | `leisure=pitch\|stadium\|playground\|golf_course` | sport |
   | `landuse=farmland\|orchard` | farm |
   | `landuse=residential\|industrial\|commercial\|retail` | built-up |

4. **Lines.** Waterways (rivers and canals wider than streams), railways, the districts' borders and the named streets
   (rank from `highway=`) become polylines, cut into the 2 km chunks. Only what lies within two tiles of the map's
   own tiles is kept, so the package does not carry the rest of Berlin.
5. **Districts.** The `admin_level=10` relations are stitched into rings. The box for each name is the largest
   lettering whose box lies inside the part of the district the map covers, around the pole of inaccessibility.
6. **Stations and bus lines.** `railway=station` nodes are markers. For each `route=bus` relation the way members are
   chained into one polyline and cut to the map; the stops are the `stop` members. `X` lines are listed as express,
   `M` lines as BRT (they also give the player's triangle its colour, `brt_lines` in the manifest), all others as
   feeders. OpenStreetMap has no timetable, so the minutes are an estimate at 18 km/h and the days are left blank.
7. **Files.** `surface.oms` is binary as in MAP_PACKAGE.md, the others are text.

## Licence

The data is (c) OpenStreetMap contributors, ODbL: see [LICENSE.md](LICENSE.md). The script is under the repository's
MIT licence. Be kind to the Overpass servers if you run it.
