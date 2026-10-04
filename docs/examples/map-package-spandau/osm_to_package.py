#!/usr/bin/env python3
"""osm_to_package.py - make an openOMSI map package from OpenStreetMap for a map laid out on web-mercator tiles.

openOMSI reads a package, it never makes one (docs/MAP_PACKAGE.md). This script is one way to make it, for maps that
have a `[worldcoordinates]` section in their global.cfg (real places, like Berlin-Spandau): there the tile index IS the
position on the globe, so OpenStreetMap lines up with the map without any fitting.

What it does, in order:
  1. reads the map's global.cfg: which tiles the map has, and the tile size openOMSI lays them out with;
  2. downloads what OpenStreetMap has there, block by block (Overpass API; every answer is kept in --cache, so a
     second run asks for nothing);
  3. turns it into the files of the package:
       surface.oms   water, green areas, rivers, rail, borders, place names; named streets (looked up, not drawn)
       regions.cfg   the districts: outline and the box their name fits in
       markers.cfg   the stations, with an icon
       lines.lin     the bus routes
       manifest.cfg  format, grid, credit
  Nothing else is needed: no third-party package, only Python 3.

Usage:
  python osm_to_package.py --map-dir "<OMSI 2>/maps/Berlin-Spandau" --out openomsi-realmap --cache osm-cache \\
      --icons icons --area-name Spandau

The data is (c) OpenStreetMap contributors, ODbL: keep the credit in manifest.cfg.
Be kind to the Overpass servers: the script waits between requests and caches everything.
"""
import argparse, hashlib, json, math, os, struct, sys, time, urllib.error, urllib.parse, urllib.request

OVERPASS = "https://overpass-api.de/api/interpreter"
AGENT = "openOMSI-map-package-example/1.0 (https://github.com/openOMSI-Project/openOMSI)"
CHUNK = 2000.0          # metres, the chunk edge of surface.oms
BLOCK = 8               # tiles on a side of the blocks the map is downloaded in

# ---------------------------------------------------------------------------- the map's grid


def read_global(path):
    raw = open(path, "rb").read()
    text = raw.decode("utf-16") if raw[:2] in (b"\xff\xfe", b"\xfe\xff") else raw.decode("cp1252")
    lines = [l.strip() for l in text.replace("\r", "").split("\n")]
    tiles = []
    for i, l in enumerate(lines):
        if l.lower() == "[map]":
            try:
                tiles.append((int(lines[i + 1]), int(lines[i + 2])))
            except (ValueError, IndexError):
                pass
    world = any(l.lower() == "[worldcoordinates]" for l in lines)
    return tiles, world


def row_width(ty):
    """Width of a web-mercator zoom-16 tile row's lower edge (m), as openOMSI computes it."""
    lat = 2 * math.atan(math.exp(2 * math.pi * ty / 65536)) - math.pi / 2
    return 40075016.69 * math.cos(lat) / 65536


def tile_size(rows):
    rows = sorted(rows)
    r = rows[len(rows) // 2]
    return (row_width(r) + row_width(r + 1)) / 2


def tile_to_latlon(tx, ty, lx, ly):
    w0, w1 = row_width(ty), row_width(ty + 1)
    lon = ((tx + 32768) + lx / ((w0 + w1) / 2)) / 65536 * 360 - 180
    row = ty + ly / w1
    return math.degrees(2 * math.atan(math.exp(2 * math.pi * row / 65536)) - math.pi / 2), lon


class Grid:
    """latitude/longitude -> metres of the author's grid (tile column * grid + place in the tile)."""

    def __init__(self, tiles):
        self.tiles = set(tiles)
        self.ts = tile_size([t[1] for t in tiles])

    def to_author(self, lat, lon):
        col = (lon + 180) / 360 * 65536
        tx = math.floor(col) - 32768
        fx = col - math.floor(col)
        rowf = math.log(math.tan(math.pi / 4 + math.radians(lat) / 2)) * 65536 / (2 * math.pi)
        ty = math.floor(rowf)
        fy = rowf - ty
        w0, w1 = row_width(ty), row_width(ty + 1)
        return tx * self.ts + fx * (w0 + w1) / 2, ty * self.ts + fy * w1

    def covered(self, ax, ay, margin=1):
        tx, ty = math.floor(ax / self.ts), math.floor(ay / self.ts)
        return any((tx + dx, ty + dy) in self.tiles for dx in range(-margin, margin + 1) for dy in range(-margin, margin + 1))


# ---------------------------------------------------------------------------- download


def overpass(query, cache, pause=6.0):
    os.makedirs(cache, exist_ok=True)
    key = os.path.join(cache, hashlib.sha1(query.encode()).hexdigest()[:16] + ".json")
    if os.path.isfile(key):
        return json.load(open(key, encoding="utf-8"))
    for attempt in range(5):
        try:
            req = urllib.request.Request(OVERPASS, data=urllib.parse.urlencode({"data": query}).encode(), headers={"User-Agent": AGENT})
            with urllib.request.urlopen(req, timeout=180) as r:
                data = json.loads(r.read().decode("utf-8"))
            json.dump(data, open(key, "w", encoding="utf-8"))
            time.sleep(pause)
            return data
        except (urllib.error.HTTPError, urllib.error.URLError, TimeoutError, json.JSONDecodeError) as e:
            wait = 30 * (attempt + 1)
            print(f"  overpass: {e}; waiting {wait} s", file=sys.stderr)
            time.sleep(wait)
    raise SystemExit("Overpass did not answer; try again later")


STREETS = "motorway|trunk|primary|secondary|tertiary|unclassified|residential|living_street|motorway_link|trunk_link|primary_link|secondary_link|tertiary_link"


def block_query(s, w, n, e):
    bb = f"({s:.6f},{w:.6f},{n:.6f},{e:.6f})"
    return f"""[out:json][timeout:120];
(
  way["natural"~"^(water|wetland|wood|scrub|grassland|heath)$"]{bb};
  relation["natural"="water"]{bb};
  way["waterway"~"^(river|stream|canal|drain)$"]{bb};
  way["leisure"~"^(park|garden|pitch|stadium|golf_course|playground|recreation_ground)$"]{bb};
  way["landuse"~"^(forest|grass|meadow|recreation_ground|cemetery|industrial|commercial|retail|residential|farmland|allotments|orchard|reservoir|basin)$"]{bb};
  way["railway"~"^(rail|light_rail|subway|tram)$"]{bb};
  way["highway"~"^({STREETS})$"]["name"]{bb};
  node["place"~"^(suburb|neighbourhood|quarter|town|city|village)$"]{bb};
  node["railway"="station"]{bb};
);
out geom;"""


def region_query(s, w, n, e, level):
    return f"""[out:json][timeout:180];
relation["boundary"="administrative"]["admin_level"="{level}"]({s:.6f},{w:.6f},{n:.6f},{e:.6f});
out geom;"""


def route_query(area):
    return f"""[out:json][timeout:240];
area["name"="{area}"]["boundary"="administrative"]->.a;
relation["route"="bus"](area.a);
out geom;"""


# ---------------------------------------------------------------------------- geometry helpers


def stitch(ways):
    """Chain ways (lists of (lat, lon)) end to end by their end points: closed rings, and what stays open."""
    key = lambda p: (round(p[0], 7), round(p[1], 7))
    pool = [list(w) for w in ways if len(w) >= 2]
    rings, open_ = [], []
    while pool:
        cur = pool.pop()
        grown = True
        while grown and key(cur[0]) != key(cur[-1]):
            grown = False
            for i, w in enumerate(pool):
                if key(w[0]) == key(cur[-1]):
                    cur += w[1:]
                elif key(w[-1]) == key(cur[-1]):
                    cur += w[::-1][1:]
                elif key(w[-1]) == key(cur[0]):
                    cur = w + cur[1:]
                elif key(w[0]) == key(cur[0]):
                    cur = w[::-1] + cur[1:]
                else:
                    continue
                pool.pop(i)
                grown = True
                break
        (rings if key(cur[0]) == key(cur[-1]) and len(cur) >= 4 else open_).append(cur)
    return rings, open_


def rdp(pts, tol):
    if len(pts) < 3:
        return pts
    keep = [False] * len(pts)
    keep[0] = keep[-1] = True
    stack = [(0, len(pts) - 1)]
    while stack:
        a, b = stack.pop()
        ax, ay = pts[a]
        bx, by = pts[b]
        dx, dy = bx - ax, by - ay
        l = math.hypot(dx, dy) or 1e-9
        worst, wi = 0.0, 0
        for k in range(a + 1, b):
            d = abs((pts[k][0] - ax) * dy - (pts[k][1] - ay) * dx) / l
            if d > worst:
                worst, wi = d, k
        if worst > tol:
            keep[wi] = True
            stack += [(a, wi), (wi, b)]
    return [p for p, k in zip(pts, keep) if k]


def rdp_ring(pts, tol):
    """Douglas-Peucker for a closed ring. It is cut in two at the point farthest from the first one: the plain version
    takes the two (equal) ends for a segment of no length and would drop the whole ring."""
    ring = pts[:-1] if len(pts) > 1 and pts[0] == pts[-1] else list(pts)
    if len(ring) < 4:
        return ring + ring[:1]
    m = max(range(1, len(ring)), key=lambda k: (ring[k][0] - ring[0][0]) ** 2 + (ring[k][1] - ring[0][1]) ** 2)
    return rdp(ring[:m + 1], tol)[:-1] + rdp(ring[m:] + [ring[0]], tol)


def clip_rect(poly, x0, y0, x1, y1):
    """Sutherland-Hodgman: the part of a polygon inside a rectangle."""
    def step(pts, inside_, cross):
        out = []
        for i, p in enumerate(pts):
            q = pts[i - 1]
            if inside_(p):
                if not inside_(q):
                    out.append(cross(q, p))
                out.append(p)
            elif inside_(q):
                out.append(cross(q, p))
        return out
    cx = lambda xv: (lambda a, b: (xv, a[1] + (b[1] - a[1]) * (xv - a[0]) / (b[0] - a[0])))
    cy = lambda yv: (lambda a, b: (a[0] + (b[0] - a[0]) * (yv - a[1]) / (b[1] - a[1]), yv))
    pts = list(poly)
    for inside_, cross in ((lambda p: p[0] >= x0, cx(x0)), (lambda p: p[0] <= x1, cx(x1)), (lambda p: p[1] >= y0, cy(y0)), (lambda p: p[1] <= y1, cy(y1))):
        if not pts:
            break
        pts = step(pts, inside_, cross)
    return pts


def area2(p):
    return sum(p[i][0] * p[(i + 1) % len(p)][1] - p[(i + 1) % len(p)][0] * p[i][1] for i in range(len(p)))


def in_tri(a, b, c, p):
    d1 = (p[0] - b[0]) * (a[1] - b[1]) - (a[0] - b[0]) * (p[1] - b[1])
    d2 = (p[0] - c[0]) * (b[1] - c[1]) - (b[0] - c[0]) * (p[1] - c[1])
    d3 = (p[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (p[1] - a[1])
    return not ((d1 < 0 or d2 < 0 or d3 < 0) and (d1 > 0 or d2 > 0 or d3 > 0))


def triangulate(poly):
    """Ear clipping of a simple polygon (no holes): a list of triangles (x0,y0,x1,y1,x2,y2), counter-clockwise."""
    p = list(poly)
    if len(p) > 1 and p[0] == p[-1]:
        p.pop()
    q = [p[0]] if p else []
    for v in p[1:]:
        if v != q[-1]:
            q.append(v)
    p = q
    if len(p) < 3:
        return []
    if area2(p) < 0:
        p.reverse()
    idx, tris, guard = list(range(len(p))), [], 0
    while len(idx) > 3 and guard < 100000:
        guard += 1
        n, found = len(idx), False
        for k in range(n):
            i0, i1, i2 = idx[k - 1], idx[k], idx[(k + 1) % n]
            a, b, c = p[i0], p[i1], p[i2]
            if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) <= 1e-9:
                continue
            if any(in_tri(a, b, c, p[j]) for j in idx if j not in (i0, i1, i2) and p[j] not in (a, b, c)):
                continue
            tris.append((a[0], a[1], b[0], b[1], c[0], c[1]))
            idx.pop(k)
            found = True
            break
        if not found:
            idx.pop(0)       # not a simple polygon: drop its worst vertex rather than stall
    if len(idx) == 3:
        a, b, c = (p[i] for i in idx)
        if (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]) > 1e-9:
            tris.append((a[0], a[1], b[0], b[1], c[0], c[1]))
    return tris


def inside(poly, x, y):
    ins, j = False, len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        if (yi > y) != (yj > y) and x < (xj - xi) * (y - yi) / (yj - yi) + xi:
            ins = not ins
        j = i
    return ins


def edge_dist(poly, x, y):
    d, j = 1e30, len(poly) - 1
    for i in range(len(poly)):
        xi, yi = poly[i]
        xj, yj = poly[j]
        dx, dy = xj - xi, yj - yi
        l2 = dx * dx + dy * dy
        t = max(0.0, min(1.0, ((x - xi) * dx + (y - yi) * dy) / l2)) if l2 > 1e-9 else 0.0
        d = min(d, (xi + t * dx - x) ** 2 + (yi + t * dy - y) ** 2)
        j = i
    return math.sqrt(d)


def label_box(poly, name):
    """Centre and size of the box the district's name fits in: the largest letters whose box lies inside the outline,
    around the point farthest from the edge (the pole of inaccessibility). Letter widths are approximated (0.72 of the
    letter size for capitals, with a little spacing), as the game scales the text to the box anyway."""
    xs, ys = [p[0] for p in poly], [p[1] for p in poly]
    x0, x1, y0, y1 = min(xs), max(xs), min(ys), max(ys)
    cel = max(x1 - x0, y1 - y0) / 48
    best = None
    for gy in range(49):
        for gx in range(49):
            x, y = x0 + (gx + 0.5) * cel, y0 + (gy + 0.5) * cel
            if x <= x1 and y <= y1 and inside(poly, x, y):
                d = edge_dist(poly, x, y)
                if best is None or d > best[2]:
                    best = [x, y, d]
    if not best:
        return None
    step = cel * 0.5
    for _ in range(40):
        if step <= 0.5:
            break
        moved = False
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if (dx or dy):
                    x, y = best[0] + dx * step, best[1] + dy * step
                    if inside(poly, x, y) and edge_dist(poly, x, y) > best[2]:
                        best = [x, y, edge_dist(poly, x, y)]
                        moved = True
        if not moved:
            step *= 0.5
    w1 = len(name) * 0.72 * 1.05

    def fits(cx, cy, w, h):
        hw, hh = w / 2, h / 2
        pts = []
        for k in range(13):
            t = -1 + 2 * k / 12
            pts += [(cx + t * hw, cy - hh), (cx + t * hw, cy + hh), (cx - hw, cy + t * hh), (cx + hw, cy + t * hh)]
        return all(inside(poly, x, y) for x, y in pts)

    lo, hi = 0.0, min(2.4 * best[2] / w1, best[2] * 2)
    for _ in range(24):
        s = (lo + hi) / 2
        if s > 1 and fits(best[0], best[1], w1 * s * 1.28, s * 1.28):
            lo = s
        else:
            hi = s
    return (best[0], best[1], w1 * lo, lo) if lo > 1 else None


# ---------------------------------------------------------------------------- classes

FILL_WATER, FILL_GREEN, FILL_BUILT, FILL_GRASS, FILL_FOREST, FILL_SPORT, FILL_FARM = 0, 1, 2, 3, 4, 5, 6
LINE_RIVER, LINE_RAIL, LINE_BORDER, LINE_STREET = 0, 1, 2, 3


def fill_class(tags):
    n, l, lu = tags.get("natural"), tags.get("leisure"), tags.get("landuse")
    if n in ("water", "wetland") or lu in ("reservoir", "basin"):
        return FILL_WATER
    if n in ("wood",) or lu == "forest":
        return FILL_FOREST
    if n in ("scrub", "grassland", "heath") or lu in ("grass", "meadow", "allotments"):
        return FILL_GRASS
    if l in ("pitch", "stadium", "playground", "golf_course"):
        return FILL_SPORT
    if lu in ("farmland", "orchard"):
        return FILL_FARM
    if l in ("park", "garden", "recreation_ground") or lu in ("recreation_ground", "cemetery"):
        return FILL_GREEN
    if lu in ("industrial", "commercial", "retail", "residential"):
        return FILL_BUILT
    return None


RANK = {"motorway": 0, "motorway_link": 5, "trunk": 1, "trunk_link": 5, "primary": 1, "primary_link": 5, "secondary": 2,
        "secondary_link": 5, "tertiary": 3, "tertiary_link": 5, "unclassified": 4, "residential": 4, "living_street": 4}
PLACE_RANK = {"city": 255, "town": 200, "quarter": 140, "suburb": 120, "neighbourhood": 100, "village": 160}


# ---------------------------------------------------------------------------- the package


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--map-dir", required=True, help="the map's folder (with global.cfg)")
    ap.add_argument("--out", required=True, help="where the package goes (the openomsi-realmap folder)")
    ap.add_argument("--cache", default="osm-cache", help="where OpenStreetMap's answers are kept")
    ap.add_argument("--icons", help="a folder with terminal.png (copied to the package's icons/)")
    ap.add_argument("--name", help="the map's name for the manifest (default: its folder name)")
    ap.add_argument("--area-name", help="name of the administrative area whose bus routes are taken (omit for none)")
    ap.add_argument("--district-level", type=int, default=10, help="admin_level of the regions (10: Berlin's Ortsteile)")
    a = ap.parse_args()

    tiles, world = read_global(os.path.join(a.map_dir, "global.cfg"))
    if not tiles or not world:
        raise SystemExit("global.cfg has no tiles or no [worldcoordinates]: this generator is for maps on web-mercator tiles")
    g = Grid(tiles)
    print(f"{len(tiles)} tiles, tile size {g.ts:.3f} m")
    xs, ys = [t[0] for t in tiles], [t[1] for t in tiles]
    south, west = tile_to_latlon(min(xs) - 1, min(ys) - 1, 0, 0)
    north, east = tile_to_latlon(max(xs) + 2, max(ys) + 2, 0, 0)
    origin = (min(xs) * g.ts, min(ys) * g.ts)

    def au(lat, lon):          # author-grid metres, relative to the origin
        x, y = g.to_author(lat, lon)
        return x - origin[0], y - origin[1]

    # --- 1. the blocks that have a tile
    blocks = sorted({(tx // BLOCK, ty // BLOCK) for tx, ty in tiles})
    elements = {}
    for n, (bx, by) in enumerate(blocks, 1):
        s, w = tile_to_latlon(bx * BLOCK - 1, by * BLOCK - 1, 0, 0)
        nn, e = tile_to_latlon((bx + 1) * BLOCK + 1, (by + 1) * BLOCK + 1, 0, 0)
        print(f"block {n}/{len(blocks)} ...", flush=True)
        for el in overpass(block_query(s, w, nn, e), a.cache).get("elements", []):
            elements[(el["type"], el["id"])] = el
    print(len(elements), "elements")

    fills = {}                      # chunk -> class -> flat triangle list
    lines = {}                      # chunk -> list of (class, flags, width, name, pts)
    labels, markers = {}, []
    names, name_ix = [], {}

    def nid(s):
        if not s:
            return 0xFFFFFFFF
        if s not in name_ix:
            name_ix[s] = len(names)
            names.append(s)
        return name_ix[s]

    def chunk(x, y):
        return (math.floor(x / CHUNK), math.floor(y / CHUNK))

    def cut(pts):
        """Split a polyline (author metres) into pieces by chunk; each keeps the joint point of the next."""
        out, cur, k = [], [pts[0]], None
        for p, q in zip(pts, pts[1:]):
            kk = chunk((p[0] + q[0]) / 2, (p[1] + q[1]) / 2)
            if k is None:
                k = kk
            if kk != k:
                out.append((k, cur))
                cur, k = [p], kk
            cur.append(q)
        out.append((k if k is not None else chunk(*pts[0]), cur))
        return [(k, c) for k, c in out if len(c) >= 2]

    cov = lambda p: g.covered(p[0] + origin[0], p[1] + origin[1], 2)   # within two tiles of the map's own

    def runs_in_map(pts):
        out, cur = [], []
        for p in pts:
            if cov(p):
                cur.append(p)
            elif cur:
                out.append(cur)
                cur = []
        if cur:
            out.append(cur)
        return out

    def add_polygon(cls, ring):
        pts = rdp_ring([au(la, lo) for la, lo in ring], 3.0)
        if len(pts) > 700:
            pts = rdp_ring(pts, 8.0)
        if not any(cov(p) for p in pts):
            return
        for t in triangulate(pts):
            fills.setdefault(chunk((t[0] + t[2] + t[4]) / 3, (t[1] + t[3] + t[5]) / 3), {}).setdefault(cls, []).extend(t)

    def add_line(cls, flags, width, name, ring):
        pts = rdp([au(la, lo) for la, lo in ring], 2.0)
        for run in runs_in_map(pts):
            if len(run) >= 2:
                for k, c in cut(run):
                    lines.setdefault(k, []).append((cls, flags, width, nid(name), c))

    geom = lambda el: [(p["lat"], p["lon"]) for p in el.get("geometry", [])]
    for (typ, _), el in elements.items():
        t = el.get("tags", {})
        if typ == "node":
            p = (el["lat"], el["lon"])
            if t.get("place") and t.get("name"):
                x, y = au(*p)
                labels.setdefault(chunk(x, y), []).append((0, PLACE_RANK.get(t["place"], 100), nid(t["name"]), x, y))
            elif t.get("railway") == "station" and t.get("name"):
                x, y = au(*p)
                if g.covered(x + origin[0], y + origin[1], 0):
                    markers.append((t["name"], x + origin[0], y + origin[1]))
                    labels.setdefault(chunk(x, y), []).append((1, 220, nid(t["name"]), x, y))
            continue
        cls = fill_class(t)
        if typ == "way":
            pts = geom(el)
            if len(pts) < 2:
                continue
            if t.get("waterway"):
                add_line(LINE_RIVER, 0, 6.0 if t["waterway"] in ("river", "canal") else 3.0, None, pts)
            elif t.get("railway"):
                add_line(LINE_RAIL, 0, 4.0, None, pts)
            elif t.get("highway"):
                add_line(LINE_STREET, RANK.get(t["highway"], 4), 0.0, t.get("name"), pts)
            elif cls is not None and pts[0] == pts[-1] and len(pts) >= 4:
                add_polygon(cls, pts)
        elif typ == "relation" and t.get("natural") == "water" and cls == FILL_WATER:
            rings, _ = stitch([geom(m) for m in el.get("members", []) if m.get("type") == "way" and m.get("role") == "outer"])
            for r in rings:
                add_polygon(FILL_WATER, r)

    # --- 2. districts: outline, border lines, the box of the name
    regions, border_seen = [], set()
    dist = overpass(region_query(south, west, north, east, a.district_level), a.cache).get("elements", [])
    for el in dist:
        nm = el.get("tags", {}).get("name")
        outer = [[(p["lat"], p["lon"]) for p in m.get("geometry", [])] for m in el.get("members", []) if m.get("type") == "way" and m.get("role") == "outer"]
        rings, rest = stitch(outer)
        for w in outer + rest:
            k = (round(w[0][0], 6), round(w[0][1], 6), round(w[-1][0], 6), round(w[-1][1], 6)) if w else None
            if k and k not in border_seen and len(w) >= 2:
                border_seen.add(k)
                add_line(LINE_BORDER, 0, 3.0, None, w)
        if nm and rings:
            ring = max(rings, key=lambda r: abs(area2([au(*p) for p in r])))
            poly = rdp_ring([au(la, lo) for la, lo in ring], 8.0)
            # the name goes in the part of the district the map's tiles cover
            vis = clip_rect(poly, 0, 0, (max(xs) + 1 - min(xs)) * g.ts, (max(ys) + 1 - min(ys)) * g.ts)
            box = label_box(vis, nm) if len(vis) >= 3 else None
            if box and not g.covered(box[0] + origin[0], box[1] + origin[1], 3):
                box = None          # no tile of the map there: no name in the void
            regions.append((nm, poly, box))
    print(len(regions), "regions")

    # --- 3. bus routes
    routes = {}
    if a.area_name:
        for el in overpass(route_query(a.area_name), a.cache).get("elements", []):
            t = el.get("tags", {})
            ref = t.get("ref")
            if not ref:
                continue
            ways = [[(p["lat"], p["lon"]) for p in m.get("geometry", [])] for m in el.get("members", []) if m.get("type") == "way" and m.get("geometry")]
            chain = []
            for w in ways:
                if not chain:
                    chain = list(w)
                    continue
                d_end = (w[0][0] - chain[-1][0]) ** 2 + (w[0][1] - chain[-1][1]) ** 2
                d_rev = (w[-1][0] - chain[-1][0]) ** 2 + (w[-1][1] - chain[-1][1]) ** 2
                chain += (w if d_end <= d_rev else w[::-1])
            stops = [(m["lat"], m["lon"]) for m in el.get("members", []) if m.get("type") == "node" and str(m.get("role", "")).startswith("stop") and "lat" in m]
            way = [au(*p) for p in chain]
            # keep the longest run inside the map's tiles
            runs, cur = [], []
            for p in way:
                if g.covered(p[0] + origin[0], p[1] + origin[1], 1):
                    cur.append(p)
                elif cur:
                    runs.append(cur)
                    cur = []
            if cur:
                runs.append(cur)
            if not runs:
                continue
            run = rdp(max(runs, key=len), 2.0)
            km = sum(math.hypot(b[0] - a_[0], b[1] - a_[1]) for a_, b in zip(run, run[1:])) / 1000
            if km < 1.0:
                continue
            st = [au(*p) for p in stops]
            st = [p for p in st if g.covered(p[0] + origin[0], p[1] + origin[1], 1)]
            routes.setdefault(ref, []).append(dict(km=km, stops=st, way=run, name=t.get("name", ""), frm=t.get("from", ""), to=t.get("to", ""), op=t.get("operator", "")))
    print(len(routes), "bus lines")

    # --- 4. write the package
    os.makedirs(a.out, exist_ok=True)
    ox, oy = origin
    bodies = {}
    for k in sorted(set(fills) | set(lines) | set(labels)):
        b = bytearray()
        f = fills.get(k, {})
        b += struct.pack("<I", len(f))
        for cls, tri in sorted(f.items()):
            b += struct.pack("<BI", cls, len(tri) // 6) + struct.pack("<%df" % len(tri), *tri)
        ls = lines.get(k, [])
        b += struct.pack("<I", len(ls))
        for cls, flags, width, name, pts in ls:
            flat = [v for p in pts for v in p]
            b += struct.pack("<BBfII", cls, flags, width, name, len(pts)) + struct.pack("<%df" % len(flat), *flat)
        lb = labels.get(k, [])
        b += struct.pack("<I", len(lb))
        for kind, rank, name, x, y in lb:
            b += struct.pack("<BBIff", kind, rank, name, x, y)
        bodies[k] = bytes(b)
    head = bytearray(b"OMSF") + struct.pack("<I", 1) + struct.pack("<dd", ox, oy)
    allx = [k[0] for k in bodies]
    ally = [k[1] for k in bodies]
    head += struct.pack("<ffff", min(allx) * CHUNK, (max(allx) + 1) * CHUNK, min(ally) * CHUNK, (max(ally) + 1) * CHUNK) + struct.pack("<f", CHUNK)
    head += struct.pack("<I", len(names))
    for n in names:
        e = n.encode("utf-8")
        head += struct.pack("<H", len(e)) + e
    order = sorted(bodies)
    head += struct.pack("<I", len(order))
    pos, idx = len(head) + 20 * len(order), bytearray()
    for k in order:
        idx += struct.pack("<iiQI", k[0], k[1], pos, len(bodies[k]))
        pos += len(bodies[k])
    open(os.path.join(a.out, "surface.oms"), "wb").write(bytes(head) + bytes(idx) + b"".join(bodies[k] for k in order))

    reg = ["# region = name | label x | label y | label box width m | label box height m | polygon x,y x,y ...  (metres of the author's grid)"]
    for nm, poly, box in regions:
        cx, cy, bw, bh = (box[0] + ox, box[1] + oy, box[2], box[3]) if box else (0.0, 0.0, 0.0, 0.0)
        reg.append("region = %s | %.1f | %.1f | %.1f | %.1f | %s" % (nm, cx, cy, bw, bh, " ".join("%.1f,%.1f" % (p[0] + ox, p[1] + oy) for p in poly)))
    open(os.path.join(a.out, "regions.cfg"), "w", encoding="utf-8", newline="\n").write("\n".join(reg) + "\n")

    if a.icons and os.path.isfile(os.path.join(a.icons, "terminal.png")):
        os.makedirs(os.path.join(a.out, "icons"), exist_ok=True)
        with open(os.path.join(a.icons, "terminal.png"), "rb") as s_, open(os.path.join(a.out, "icons", "terminal.png"), "wb") as d_:
            d_.write(s_.read())
        mk = ["# icon | width in pixels at interface size 1 | x | y | name  (x, y: metres of the author's grid)"]
        seen = set()
        for nm, x, y in markers:
            if (nm, round(x), round(y)) not in seen:
                seen.add((nm, round(x), round(y)))
                mk.append("marker = icons/terminal.png | 22 | %.1f | %.1f | %s" % (x, y, nm))
        open(os.path.join(a.out, "markers.cfg"), "w", encoding="utf-8", newline="\n").write("\n".join(mk) + "\n")

    if routes:
        out = ["RLIN\t1", "# made by osm_to_package.py from OpenStreetMap route relations; the minutes are an estimate at 18 km/h", "ORIGEM\t%.1f\t%.1f" % (ox, oy)]
        for ref in sorted(routes, key=lambda r: (not r[:1].isalpha(), r)):
            rs = sorted(routes[ref], key=lambda r: r["name"])[:2]
            group = "EXP" if ref.upper().startswith("X") else ("BRT" if ref.upper().startswith("M") else "ALI")
            out.append("L\t%s\t%s\t%s" % (ref, group, (rs[0]["to"] or rs[0]["name"] or ref)))
            for r in rs:
                w = r["way"]
                circ = 1 if math.hypot(w[0][0] - w[-1][0], w[0][1] - w[-1][1]) < 150 else 0
                itin = (r["frm"] + " - " + r["to"]) if r["frm"] and r["to"] else (r["name"] or ref)
                out.append("R\t%.3f\t%d\t%d\t%d\t%s\t%s\t%s" % (r["km"], len(r["stops"]), round(r["km"] / 18 * 60), circ, "-", r["op"] or "Bus", itin))
                out.append("P\t" + " ".join("%.1f %.1f" % p for p in w))
                out.append("S\t" + " ".join("%.1f %.1f" % p for p in r["stops"]))
        open(os.path.join(a.out, "lines.lin"), "w", encoding="utf-8", newline="\n").write("\n".join(out) + "\n")

    metro = ", ".join(sorted(r for r in routes if r.upper().startswith("M")))
    man = f"""# openOMSI map package (docs/MAP_PACKAGE.md), made by osm_to_package.py
format    = 1
name      = {a.name or os.path.basename(os.path.normpath(a.map_dir))}
# the tile size openOMSI lays this map out with, in metres
grid      = {g.ts:.3f}
credit    = (c) OpenStreetMap contributors (ODbL)

player      = triangle
player_free = #ffffff
player_line = #5588c7
player_brt  = #1e7b34
brt_lines   = {metro}
"""
    open(os.path.join(a.out, "manifest.cfg"), "w", encoding="utf-8", newline="\n").write(man)
    size = sum(os.path.getsize(os.path.join(r, f)) for r, _, fs in os.walk(a.out) for f in fs)
    print(f"{a.out}: {size / 1e6:.2f} MB, {len(bodies)} chunks, {len(names)} names, {len(regions)} regions, {len(markers)} stations, {len(routes)} lines")


if __name__ == "__main__":
    main()
