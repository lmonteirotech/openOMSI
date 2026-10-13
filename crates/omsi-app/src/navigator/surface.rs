//! A map's package drawn under the roads (`omsi_map::package`): the ground, areas of water, green
//! land and built-up land, then rivers, rail and borders, in the package's own colours.

use super::*;
use omsi_map::package::{self, Package, Rgb};
use omsi_ui::Vertex;

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

impl Navigator {
    /// The map's package, given at once (an offscreen picture reads it on its own thread).
    pub fn set_surface(&mut self, surface: Option<Package>) {
        if let Some(s) = surface {
            self.surface = Some(std::sync::Arc::new(s));
            self.surface_version += 1;
        }
    }

    /// The small map's surface (buffer 6), built again when it arrives, the look changes or the anchor moved:
    /// its new vertices, or `None` when the buffer is as it was.
    pub(super) fn minimap_surface(&mut self, anchor: DVec2) -> Option<Vec<Vertex>> {
        let sf = self.surface.clone().filter(|_| self.show_surface)?;
        let key = self.surface_version * 2 + self.light as u64;
        if self.surface_mesh.is_some_and(|m| m.0 == key && m.1 == anchor) {
            return None;
        }
        let mut p = Painter::new();
        let t0 = std::time::Instant::now();
        build_surface(&mut p, &sf, anchor, Some((anchor, ROAD_RADIUS * 1.4)), self.light);
        if omsi_cfg::flags::OMSI_DEBUG_NAV.is_set() {
            log::info!("navigator: surface around ({:.0}, {:.0}) in {:.1} ms: {} vertices", anchor.x, anchor.y, t0.elapsed().as_secs_f64() * 1000.0, p.verts.len());
        }
        self.surface_mesh = Some((key, anchor, p.len()));
        Some(p.verts)
    }

    /// The city map's surface (buffer 7), built once per surface, look and anchor: its new vertices, or `None`.
    pub(super) fn city_surface(&mut self, anchor: DVec2) -> Option<Vec<Vertex>> {
        let sf = self.surface.clone().filter(|_| self.show_surface)?;
        let key = self.surface_version * 2 + self.light as u64;
        if self.city.surface.is_some_and(|m| m.0 == key && m.2 == anchor) {
            return None;
        }
        let mut p = Painter::new();
        let t0 = std::time::Instant::now();
        build_surface(&mut p, &sf, anchor, None, self.light);
        if omsi_cfg::flags::OMSI_DEBUG_NAV.is_set() {
            log::info!("navigator: the city map's surface in {:.1} ms: {} vertices", t0.elapsed().as_secs_f64() * 1000.0, p.verts.len());
        }
        self.city.surface = Some((key, p.len(), anchor));
        Some(p.verts)
    }

    /// The ground of the city map: the package's own colour (dimmed by the opacity setting), or the plain dark one.
    pub(super) fn city_ground(&self) -> Color {
        let a = crate::ui::backdrop(self.opacity).min(1.0);
        match (self.show_surface, self.surface.as_ref()) {
            (true, Some(sf)) => ground(sf, self.light).alpha(a),
            _ => Color::rgba(18, 18, 18, a),
        }
    }

    /// The data's attribution (the package's licence asks for it), bottom right of the city map.
    pub(super) fn draw_credit(&mut self, ui: &mut Painter, w: f32, h: f32, s: f32) {
        if let (true, Some(sf)) = (self.show_surface, self.surface.as_ref().filter(|s| !s.credit.is_empty())) {
            ui.text(&mut self.atlas, &self.fonts, &sf.credit, 10.5 * s, Weight::Medium, Vec2::new(w - 12.0 * s, h - 8.0 * s), Align::Right, if self.light { Color::hex(0x5b564c).alpha(0.8) } else { TEXT_DIM.alpha(0.7) });
        }
    }
}
