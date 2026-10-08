//! What the simulation did that the renderer has to follow.
//!
//! The simulation does not touch the renderer. What it does that the renderer has to
//! follow - somebody appearing, somebody going, coins put on the desk - it writes down as
//! [`BodyOp`]s, and omsi-app's view replays them in the order they happened, at the end of
//! the same call that made them: the renderer sees the very calls, in the very order, it
//! saw when the simulation made them itself.

use super::*;

/// Something the simulation did that the renderer has to follow.
pub enum BodyOp {
    /// Somebody appeared: the meshes of their type and clothes, at `position`.
    Spawn { id: u32, ty: Arc<HumanType>, variant: usize, position: DVec3 },
    /// Somebody went: hidden, and their meshes kept for the next person of the type.
    /// `meshes` are theirs as they were when they went (none yet for somebody who came
    /// and went before the view caught up).
    Retire { id: u32, tkey: usize, variant: usize, meshes: Vec<(usize, usize)> },
    /// Coins on a point of the player's cabin (`Money::place`).
    Coins { coins: Vec<usize>, point: Vec3, var: [f32; 2], change: bool, parent: Option<String> },
}

impl BodyOp {
    /// Whether replaying it needs the world (textures, the money's meshes).
    pub fn needs_world(&self) -> bool {
        !matches!(self, BodyOp::Retire { .. })
    }
}

/// The simulation's side of the people's bodies: what the view has to catch up with.
#[derive(Default)]
pub struct BodyOps {
    /// What the simulation did since the view last caught up, oldest first.
    pub ops: Vec<BodyOp>,
}

impl BodyOps {
    /// Somebody not yet drawn is now called `to` (a LAN host's person takes the host's id).
    pub fn renamed(&mut self, from: u32, to: u32) {
        for op in self.ops.iter_mut() {
            if let BodyOp::Spawn { id, .. } = op {
                if *id == from {
                    *id = to;
                }
            }
        }
    }
}

impl PeopleSim {
    /// Somebody has gone: hidden, and their meshes kept for the next person of the type.
    pub fn retire(&mut self, p: &Person) {
        let tkey = Arc::as_ptr(&p.ty) as usize;
        self.bodies.ops.push(BodyOp::Retire { id: p.id, tkey, variant: p.variant, meshes: p.meshes.clone() });
    }
}
