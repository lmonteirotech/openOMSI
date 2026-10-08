//! The pavements as one network of paths for the pedestrians, and their crossings.

use super::*;

/// Whether the straight way from `a` to `b` goes over a carriageway: across the centre
/// line of a street lane (walking along the kerb on the carriageway's edge does not).
pub fn crosses_street(net: &Network, a: DVec2, b: DVec2) -> bool {
    let mut cells: Vec<(i32, i32)> = Vec::new();
    for p in [a, b, (a + b) * 0.5] {
        let c = Network::grid_cell(p.extend(0.0));
        if !cells.contains(&c) {
            cells.push(c);
        }
    }
    let mut seen: Vec<usize> = Vec::new();
    for c in cells {
        for &i in net.grid.get(&c).map(|v| v.as_slice()).unwrap_or(&[]) {
            if seen.contains(&i) {
                continue;
            }
            seen.push(i);
            let l = &net.lanes[i];
            if l.kind != LaneKind::Street {
                continue;
            }
            if l
                .points
                .windows(2)
                .any(|w| segments_cross(a, b, w[0].truncate(), w[1].truncate()))
            {
                return true;
            }
        }
    }
    false
}

/// Whether the segments `a`-`b` and `c`-`d` cross.
pub fn segments_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> bool {
    let side = |p: DVec2, q: DVec2, r: DVec2| (q - p).perp_dot(r - p);
    let (d1, d2) = (side(c, d, a), side(c, d, b));
    let (d3, d4) = (side(a, b, c), side(a, b, d));
    d1 * d2 < 0.0 && d3 * d4 < 0.0
}

/// One piece of a walk along a pavement path: lane `lane` from distance `a` to `b`.
#[derive(Debug, Clone, Copy)]
pub struct Leg {
    pub lane: usize,
    pub a: f32,
    pub b: f32,
}

impl Leg {
    pub fn len(&self) -> f32 {
        (self.b - self.a).abs()
    }
    pub fn dist(&self, p: f32) -> f32 {
        if self.b >= self.a {
            self.a + p
        } else {
            self.a - p
        }
    }
    /// Point and walking heading `p` metres into the leg.
    pub fn at(&self, net: &Network, p: f32) -> (DVec3, f64) {
        let (q, h) = net.lanes[self.lane].at(self.dist(p.clamp(0.0, self.len())));
        (
            q,
            if self.b >= self.a {
                h as f64
            } else {
                h as f64 + 180.0
            },
        )
    }
    /// How far into the leg the point nearest `pos` lies, looking around `hint`.
    pub fn project(&self, net: &Network, pos: DVec3, hint: f32) -> f32 {
        let (lo, hi) = ((hint - 1.5).max(0.0), (hint + 3.0).min(self.len()));
        let mut best = (hint, f64::MAX);
        let mut p = lo;
        while p <= hi + 1e-3 {
            let d = (self.at(net, p).0 - pos).truncate().length_squared();
            if d < best.1 {
                best = (p, d);
            }
            p += 0.2;
        }
        best.0
    }
    /// Whether the leg starts at an end of its lane (at a kerb or a junction).
    pub fn from_end(&self, net: &Network) -> bool {
        self.a < 0.05 || self.a > net.lanes[self.lane].length() - 0.05
    }
}

/// A walk along the pavement network.
#[derive(Debug, Clone)]
pub struct PedWalk {
    pub legs: Vec<Leg>,
    pub leg: usize,
    /// Metres walked into the current leg.
    pub s: f32,
    /// A stroll: goes on at random when the legs run out.
    pub roam: bool,
    /// Keep-right offset (m).
    pub side: f32,
    /// Seconds spent waiting at the kerb before the current leg.
    pub held: f32,
}

impl PedWalk {
    pub fn new(legs: Vec<Leg>, roam: bool, side: f32) -> PedWalk {
        PedWalk {
            legs,
            leg: 0,
            s: 0.0,
            roam,
            side,
            held: 0.0,
        }
    }
}

/// The pavement paths as a walking network: path ends closer than a metre are one
/// junction, whatever their heading (the road network joins lane ends only when they
/// continue in the same direction, which leaves every pavement corner open).
pub struct PedNet {
    /// Per pavement lane: its start and end junction.
    pub ends: HashMap<usize, (usize, usize)>,
    /// Per junction: (lane, walked forwards) leaving it.
    pub out: Vec<Vec<(usize, bool)>>,
    /// Where each pavement lane crosses a carriageway (lazily), and the carriageway lanes
    /// it crosses.
    pub crossings: HashMap<usize, Vec<DVec2>>,
    pub crossed: HashMap<usize, Vec<usize>>,
    /// Pavement lanes by 50 m cell.
    pub grid: HashMap<(i32, i32), Vec<usize>>,
    /// The junctions and a 1.5 m grid of them, for joining the paths of tiles loaded later.
    pub nodes: Vec<DVec3>,
    pub cells: HashMap<(i64, i64), Vec<usize>>,
    /// How many lanes of the traffic network are in (the network only grows: tiles bring
    /// their lanes and the indices stay).
    pub built: usize,
}

impl PedNet {
    pub fn build(net: &Network) -> PedNet {
        let mut p = PedNet {
            ends: HashMap::new(),
            out: Vec::new(),
            crossings: HashMap::new(),
            crossed: HashMap::new(),
            grid: HashMap::new(),
            nodes: Vec::new(),
            cells: HashMap::new(),
            built: 0,
        };
        p.extend(net);
        log::info!(
            "pavement network: {} paths, {} junctions",
            p.ends.len(),
            p.nodes.len()
        );
        p
    }

    /// Take in the lanes the network gained since the last call (tiles streamed in).
    pub fn extend(&mut self, net: &Network) -> usize {
        let from = self.built.min(net.lanes.len());
        let before = self.ends.len();
        for i in from..net.lanes.len() {
            let l = &net.lanes[i];
            if l.kind == LaneKind::Street && from > 0 {
                // a new carriageway may cross pavement paths that are in already
                self.crossings.clear();
            }
            if l.kind != LaneKind::Sidewalk || l.points.len() < 2 || l.length() < 0.3 {
                continue;
            }
            let a = self.node_of(l.start());
            let b = self.node_of(l.end());
            if a == b && l.length() < 3.0 {
                continue;
            }
            self.ends.insert(i, (a, b));
            self.out[a].push((i, true));
            self.out[b].push((i, false));
            let mut seen: Vec<(i32, i32)> = Vec::new();
            for p in &l.points {
                let c = ((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32);
                if !seen.contains(&c) {
                    seen.push(c);
                    self.grid.entry(c).or_default().push(i);
                }
            }
        }
        self.built = net.lanes.len();
        self.ends.len() - before
    }

    /// The junction at `p`, a new one when there is none within a metre.
    pub fn node_of(&mut self, p: DVec3) -> usize {
        let (cx, cy) = ((p.x / 1.5).floor() as i64, (p.y / 1.5).floor() as i64);
        for dx in -1..=1 {
            for dy in -1..=1 {
                if let Some(list) = self.cells.get(&(cx + dx, cy + dy)) {
                    for &n in list {
                        if (self.nodes[n] - p).truncate().length() < 1.2
                            && (self.nodes[n].z - p.z).abs() < 2.5
                        {
                            return n;
                        }
                    }
                }
            }
        }
        self.nodes.push(p);
        self.out.push(Vec::new());
        self.cells
            .entry((cx, cy))
            .or_default()
            .push(self.nodes.len() - 1);
        self.nodes.len() - 1
    }

    /// The pavement lane nearest `p` within `reach` that can be reached without going over
    /// a carriageway: (lane, distance along it, distance to it). The plain nearest one was
    /// often the pavement across the road - a passenger off a bus then walked straight
    /// over the carriageway through the traffic to it, or joined a crossing in the middle.
    pub fn nearest(&self, net: &Network, p: DVec3, reach: f64) -> Option<(usize, f32, f64)> {
        let (cx, cy) = ((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32);
        let mut cands: Vec<(usize, f32, f64)> = Vec::new();
        let mut seen = HashSet::new();
        for dx in -1..=1 {
            for dy in -1..=1 {
                for &i in self
                    .grid
                    .get(&(cx + dx, cy + dy))
                    .map(|v| v.as_slice())
                    .unwrap_or(&[])
                {
                    if !seen.insert(i) {
                        continue;
                    }
                    if let Some((s, d)) = net.lanes[i].nearest_point(p) {
                        if d < reach {
                            cands.push((i, s, d));
                        }
                    }
                }
            }
        }
        cands.sort_by(|a, b| a.2.total_cmp(&b.2));
        let first = cands.first().copied();
        cands
            .into_iter()
            .find(|&(i, s, _)| {
                let (q, _) = net.lanes[i].at(s);
                !crosses_street(net, p.truncate(), q.truncate())
                    && !self.crossings.get(&i).map(|x| !x.is_empty()).unwrap_or(false)
            })
            // on an island between carriageways: the nearest after all
            .or(first)
    }

    /// The junction a leg ends at, when it ends at one.
    pub fn end_node(&self, net: &Network, leg: &Leg) -> Option<usize> {
        let (a, b) = *self.ends.get(&leg.lane)?;
        let len = net.lanes[leg.lane].length();
        if leg.b < 0.05 {
            Some(a)
        } else if leg.b > len - 0.05 {
            Some(b)
        } else {
            None
        }
    }

    /// A leg leaving junction `node`, not back along `came` (unless it is a dead end).
    pub fn next_leg(&self, net: &Network, node: usize, came: usize, pick: u64) -> Option<Leg> {
        let back = self.ends.get(&came).copied();
        let twin = |l: usize| -> bool {
            l == came
                || matches!((self.ends.get(&l), back), (Some(&(a, b)), Some((c, d))) if a == d && b == c && (net.lanes[l].length() - net.lanes[came].length()).abs() < 1.0)
        };
        let list: Vec<(usize, bool)> = self
            .out
            .get(node)?
            .iter()
            .copied()
            .filter(|(l, _)| !twin(*l))
            .collect();
        // the way on rather than back: a path leaving the junction within 110° of the way
        // the walker came (there usually is one - a pavement goes on past a side street),
        // else any. Picked from all, a stroller would turn round at every corner and walk
        // back the way they came, which looked like a change of mind for no reason.
        let heading_in = {
            let l = &net.lanes[came];
            let (a, _) = back.unwrap_or((usize::MAX, usize::MAX));
            // arriving at `node` along `came`: forwards if its end is the node
            if a == node {
                wrap_heading(l.start_heading() as f64 + 180.0)
            } else {
                l.end_heading() as f64
            }
        };
        let leaving = |&(l, fwd): &(usize, bool)| -> f64 {
            let lane = &net.lanes[l];
            if fwd {
                lane.start_heading() as f64
            } else {
                wrap_heading(lane.end_heading() as f64 + 180.0)
            }
        };
        let onward: Vec<(usize, bool)> = list
            .iter()
            .copied()
            .filter(|o| angle_between(heading_in, leaving(o)) <= 110.0)
            .collect();
        let list = if onward.is_empty() { list } else { onward };
        let (lane, fwd) = if list.is_empty() {
            // a dead end: turn round
            let (a, _) = back?;
            (came, a == node)
        } else {
            list[(pick as usize) % list.len()]
        };
        let len = net.lanes[lane].length();
        Some(if fwd {
            Leg {
                lane,
                a: 0.0,
                b: len,
            }
        } else {
            Leg {
                lane,
                a: len,
                b: 0.0,
            }
        })
    }


    /// Where pavement lane `lane` crosses a carriageway.
    pub fn crossings(&mut self, net: &Network, lane: usize) -> &[DVec2] {
        if !self.crossings.contains_key(&lane) {
            let l = &net.lanes[lane];
            let mut cand: Vec<usize> = Vec::new();
            for p in &l.points {
                if let Some(list) = net
                    .grid
                    .get(&((p.x / 50.0).floor() as i32, (p.y / 50.0).floor() as i32))
                {
                    for &i in list {
                        if net.lanes[i].kind == LaneKind::Street && !cand.contains(&i) {
                            cand.push(i);
                        }
                    }
                }
            }
            let mut out = Vec::new();
            let mut crossed = Vec::new();
            for i in cand {
                let o = &net.lanes[i];
                for w in l.points.windows(2) {
                    for v in o.points.windows(2) {
                        if (w[0].z - v[0].z).abs() > 3.0 {
                            continue;
                        }
                        if let Some(x) = seg_cross(
                            w[0].truncate(),
                            w[1].truncate(),
                            v[0].truncate(),
                            v[1].truncate(),
                        ) {
                            if !crossed.contains(&i) {
                                crossed.push(i);
                            }
                            if !out.iter().any(|q: &DVec2| (*q - x).length() < 1.5) {
                                out.push(x);
                            }
                        }
                    }
                }
            }
            self.crossings.insert(lane, out);
            self.crossed.insert(lane, crossed);
        }
        &self.crossings[&lane]
    }

    /// The carriageway lanes a pavement lane crosses.
    pub fn crossed_lanes(&mut self, net: &Network, lane: usize) -> &[usize] {
        self.crossings(net, lane);
        &self.crossed[&lane]
    }
}

/// Seconds a pedestrian starting across `path` now has before a vehicle may drive over it:
/// until the first light of a carriageway lane it crosses (or of a lane leading into one)
/// turns green once the pedestrian green is over. Lanes that have green now, or get it
/// while the pedestrians still have theirs, are turning traffic that gives way. Without
/// such a light, the pedestrian green `green_left` and two seconds.
pub fn pedestrian_window(
    ped: Option<&mut PedNet>,
    net: &Network,
    traffic: &TrafficSim,
    path: usize,
    green_left: f32,
) -> f32 {
    let mut window = f32::MAX;
    if let Some(ped) = ped {
        for &s in ped.crossed_lanes(net, path) {
            let feeding = net.prev.get(s).map(|p| p.as_slice()).unwrap_or(&[]);
            for &l in std::iter::once(&s).chain(feeding) {
                let Some((c, li)) = net.lanes[l].traffic_light else {
                    continue;
                };
                if let Some(g) = traffic
                    .light_until_go(c, li)
                    .filter(|g| *g > 0.0 && *g >= green_left)
                {
                    window = window.min(g);
                }
            }
        }
    }
    if window == f32::MAX {
        green_left + 2.0
    } else {
        window
    }
}

pub fn seg_cross(a: DVec2, b: DVec2, c: DVec2, d: DVec2) -> Option<DVec2> {
    let r = b - a;
    let s = d - c;
    let den = r.perp_dot(s);
    if den.abs() < 1e-9 {
        return None;
    }
    let t = (c - a).perp_dot(s) / den;
    let u = (c - a).perp_dot(r) / den;
    (t >= 0.0 && t <= 1.0 && u >= 0.0 && u <= 1.0).then(|| a + r * t)
}
