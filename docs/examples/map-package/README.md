# Example map package

A real map package, made for the map **RMG Leste** (the Goiania metropolitan area, Brazil), to show
what [../../MAP_PACKAGE.md](../../MAP_PACKAGE.md) describes with real data: a surface of 292 chunks, 8 map icons, 6 municipalities,
12,380 named street pieces and 26 bus lines.

```
openomsi-realmap/
  manifest.cfg    format 1, the author's tile grid (585.795 m), colours of the player's triangle, the BRT lines
  surface.oms     water, green areas, woods, rivers, rail, borders and place names; named streets
  markers.cfg     terminals and stations (and a depot) with their icons
  regions.cfg     the six municipalities: outline and the box their name fits in
  lines.lin       the bus lines: kind, name, routes, stops, ways
  icons/          terminal.png, depot.png
```

To try it, put the `openomsi-realmap` folder in the map's folder, `Maps/RMG Leste/openomsi-realmap/`. The
positions are in that map's own tile grid, so it only means something on that map, which is not out yet;
the screenshots in the pull request show what it does. A map without the folder looks as it always did.

Made beforehand by the map's authors from OpenStreetMap and the map's own timetables; openOMSI only
reads it.

**The logos seen in the pull request's screenshots (an operator's garage logo and the transit system's mark)
are not included in this example.** They belong to the map's own package; the icons here are neutral
stand-ins, and they stay so.

Data (c) OpenStreetMap contributors, ODbL: see [LICENSE.md](LICENSE.md).
