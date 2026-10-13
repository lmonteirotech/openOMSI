//! A map's package drawn under the roads (`omsi_map::package`): the ground, areas of water, green
//! land and built-up land, then rivers, rail and borders, in the package's own colours.

use super::*;
use omsi_map::package::{self, Package, Rgb};

fn color(c: Rgb) -> Color {
    Color::rgba(c[0], c[1], c[2], 1.0)
}

/// The colour of the bare ground the map is drawn on.
pub(super) fn ground(pkg: &Package, light: bool) -> Color {
    color(pkg.palette(light).land)
}

/// The package's surface as vertices relative to `anchor`, to be drawn under the roads. `near`
/// limits it to the chunks within a radius of a point (the small map); `None` builds the whole map.
pub(super) fn build_surface(p: &mut Painter, pkg: &Package, anchor: DVec2, near: Option<(DVec2, f64)>, light: bool) {
    let rel = |q: (f64, f64)| Vec3::new((q.0 - anchor.x) as f32, (q.1 - anchor.y) as f32, 0.0);
    let area = near.map(|(c, r)| ((c.x - r, c.y - r), (c.x + r, c.y + r)));
    let layers = pkg.layers(area);
    let pal = pkg.palette(light);
    // under everything: built-up land, farmland, grass, parks, woods, sports grounds, water
    let order = [
        (package::FILL_BUILT, pal.built),
        (package::FILL_FARM, pal.farm),
        (package::FILL_GRASS, pal.grass),
        (package::FILL_GREEN, pal.green),
        (package::FILL_FOREST, pal.forest),
        (package::FILL_SPORT, pal.sport),
        (package::FILL_WATER, pal.water),
    ];
    for (class, c) in order {
        for t in &layers.fills[class as usize] {
            p.world_poly(&[rel(t[0]), rel(t[1]), rel(t[2])], color(c));
        }
    }
    // rivers under rail under borders; a river's width tells river from stream
    for want in [package::LINE_RIVER, package::LINE_RAIL, package::LINE_BORDER] {
        for l in layers.lines.iter().filter(|l| l.class == want) {
            let v: Vec<Vec3> = l.points.iter().map(|q| rel(*q)).collect();
            match l.class {
                package::LINE_RIVER if l.width >= 5.0 => p.ribbon(&v, 9.0, 2.6, color(pal.river), false),
                package::LINE_RIVER => p.ribbon(&v, 3.5, 1.5, color(pal.river), false),
                // the railway as two lines: a wide edge with the rail's own colour on it
                package::LINE_RAIL => {
                    p.ribbon(&v, 4.2, 3.2, color(pal.rail_edge), false);
                    p.ribbon(&v, 2.2, 1.6, color(pal.rail), false);
                }
                _ => p.ribbon(&v, 1.4, 1.4, color(pal.border).alpha(0.7), false),
            }
        }
    }
}
