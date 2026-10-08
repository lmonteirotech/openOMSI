//! The sun's shadow cascades of a frame: which are redrawn (and with which light
//! matrices), and what each of them draws.

use super::*;

/// The shadow cascades of a frame (see `Renderer::plan_shadow_cache`).
pub(crate) struct ShadowPlan {
    pub moon_shadows: bool,
    pub shadows: bool,
    pub draw_shadows: bool,
    pub redraw_near: bool,
    pub redraw_far: bool,
    pub light_view_proj: Mat4,
    pub light_view_proj_far: Mat4,
    pub light_view_proj_close: Mat4,
}

impl Renderer {
    /// The light matrices of the cascades, the near and far ones kept from an earlier
    /// frame while they still fit (and the second XR eye taking the first one's).
    pub(crate) fn plan_shadow_cache(&self, scene: &Scene, f: &FrameCtx) -> ShadowPlan {
        let (lighting, cam_rel, projection, second_eye) = (f.lighting, f.cam_rel, f.projection, f.second_eye);
        let enhanced_frame = f.enhanced_frame;
        // sun shadow map: an orthographic box around the camera, looking along the sun (by
        // night along the moon, see `Lighting::casts_moon_shadows`)
        let moon_shadows = enhanced_frame && lighting.casts_moon_shadows();
        let sun = if moon_shadows { lighting.moon_dir.normalize_or_zero() } else { lighting.sun_dir.normalize_or_zero() };
        let shadows = (f.with_overlays || projection.is_some()) && (lighting.casts_sun_shadows() || moon_shadows);
        let shared_xr_shadows = if second_eye && shadows {
            self.xr_shadow_cache
                .get()
                .filter(|(origin, previous_sun, _, _, _)| *origin == scene.render_origin && *previous_sun == sun)
        } else {
            None
        };
        let draw_shadows = shadows && shared_xr_shadows.is_none();
        let light_matrix = |range: f32| {
            // Snap the centre to whole texels so the map does not shimmer while driving.
            let texel = range * 2.0 / self.options.shadow_size as f32;
            let up = if sun.z.abs() > 0.95 { Vec3::Y } else { Vec3::Z };
            let raw = cam_rel;
            let view0 = Mat4::look_at_rh(sun * 900.0, Vec3::ZERO, up);
            let ls = view0.transform_point3(raw);
            let snapped = Vec3::new(
                (ls.x / texel).round() * texel,
                (ls.y / texel).round() * texel,
                ls.z,
            );
            let center = view0.inverse().transform_point3(snapped);
            let view = Mat4::look_at_rh(center + sun * 900.0, center, up);
            let proj = Mat4::orthographic_rh(-range, range, -range, range, 1.0, 2200.0);
            proj * view
        };
        // The near cascade (140 m, 4096 texels at the top setting: the costliest shadow
        // pass, a third of it the trees' leaf cards) is drawn every other frame and kept
        // for the next, with the light matrix it was drawn with; the close one - the bus
        // and everything within 30 m - every frame. Redrawn at once when the camera has
        // jumped, the sun has moved or the render origin has (its matrix is relative to it).
        let near_wanted = light_matrix(SHADOW_RANGE);
        let (near_m, near_age, near_origin, near_sun) = self.shadow_near_cache.get();
        let near_jumped = (near_m.project_point3(cam_rel) - near_wanted.project_point3(cam_rel)).length() > 0.03;
        let redraw_near = draw_shadows
            && (near_age >= 1
                || near_jumped
                || near_m == Mat4::IDENTITY
                || near_origin != scene.render_origin
                || near_sun.dot(sun) < 0.99999
                || f.env.shadow_near_every_frame);
        let light_view_proj = if let Some((_, _, near, _, _)) = shared_xr_shadows {
            near
        } else if !shadows {
            near_wanted
        } else if redraw_near {
            self.shadow_near_cache.set((near_wanted, 0, scene.render_origin, sun));
            near_wanted
        } else {
            self.shadow_near_cache.set((near_m, near_age + 1, near_origin, near_sun));
            near_m
        };
        let light_view_proj_close = shared_xr_shadows
            .map(|(_, _, _, _, close)| close)
            .unwrap_or_else(|| light_matrix(SHADOW_RANGE_CLOSE));
        // The far cascade (700 m, metre-sized texels) is drawn every 4th frame, or at once
        // when the camera has left the middle of the one drawn, the sun has moved or the
        // render origin has jumped (its matrix is relative to that). Drawn every frame it
        // was 0.6 ms of GPU time for a picture that hardly changes; a car in it is a few
        // texels, and a tile streamed in waits three frames at most for its shadow.
        let far_wanted = light_matrix(SHADOW_RANGE_FAR);
        let (far_m, far_age, far_origin, far_sun) = self.shadow_far_cache.get();
        let far_moved = (far_m.project_point3(cam_rel) - far_wanted.project_point3(cam_rel)).length() > 0.12;
        let redraw_far = draw_shadows
            && (far_age >= 3
                || far_moved
                || far_m == Mat4::IDENTITY
                || far_origin != scene.render_origin
                || far_sun.dot(sun) < 0.99999
                || f.env.shadow_far_every_frame);
        if redraw_far && f.env.debug_shadow_far {
            log::info!("far shadow redrawn: age {far_age} moved {far_moved} origin {} sun {:.6}", far_origin != scene.render_origin, far_sun.dot(sun));
        }
        // The desktop view and the first XR eye maintain this cache; mirrors and
        // the second XR eye leave it alone.
        let light_view_proj_far = if let Some((_, _, _, far, _)) = shared_xr_shadows {
            far
        } else if !shadows {
            far_wanted
        } else if redraw_far {
            self.shadow_far_cache.set((far_wanted, 0, scene.render_origin, sun));
            far_wanted
        } else {
            self.shadow_far_cache.set((far_m, far_age + 1, far_origin, far_sun));
            far_m
        };
        if projection.is_some() && !second_eye && shadows {
            self.xr_shadow_cache.set(Some((
                scene.render_origin,
                sun,
                light_view_proj,
                light_view_proj_far,
                light_view_proj_close,
            )));
        }
        ShadowPlan {
            moon_shadows,
            shadows,
            draw_shadows,
            redraw_near,
            redraw_far,
            light_view_proj,
            light_view_proj_far,
            light_view_proj_close,
        }
    }

    /// The shadow casters of each cascade (near, far, close) and of the street lamps'
    /// maps, as batches over the frame's draw list.
    pub(crate) fn plan_shadow_casters(&self, scene: &Scene, f: &FrameCtx, sh: &ShadowPlan, list: &mut Vec<u32>) -> [Vec<Batch>; SHADOW_LISTS] {
        let (camera, cam_rel, rt_frame) = (f.camera, f.cam_rel, f.rt_frame);
        let (moon_shadows, draw_shadows, redraw_near, redraw_far) = (sh.moon_shadows, sh.draw_shadows, sh.redraw_near, sh.redraw_far);
        let (light_view_proj, light_view_proj_far, light_view_proj_close) = (sh.light_view_proj, sh.light_view_proj_far, sh.light_view_proj_close);
        let lamp_shadows = &f.lamp_shadows;
        let debug_draws = f.env.debug_draws;
        // near, far, close
        // (3..: each street lamp's map, every caster within that lamp's reach under its head;
        // a source embedded in a pole's fixture leaves its own fixture out, see `PointLight::shadow_owner`)
        let mut shadow_batches: [Vec<Batch>; SHADOW_LISTS] = std::array::from_fn(|_| Vec::new());
        let lamp_reaches = |l: &LampShadow, c: Vec3, r: f32| c.z - r < l.position.z && (c - l.position).length() < l.range + r;
        let lamp_reach = |c: Vec3, r: f32| lamp_shadows.iter().any(|l| lamp_reaches(l, c, r));
        // an instance's screen size as the camera pass measures it for the LOD choice
        let lod_fov = camera.fov_deg.to_radians().max(1e-3);
        let lod_size = |inst: &Instance| -> f32 {
            let scale = Self::instance_scale(scene, inst);
            let radius = if inst.object_radius > 0.0 { inst.object_radius } else { scene.meshes[inst.mesh].bounds_radius } * scale;
            let d = ((inst.origin - scene.render_origin).as_vec3() - cam_rel).length();
            if d <= radius { f32::MAX } else { 2.0 * radius / (d.max(0.01) * lod_fov) }
        };
        let active = [draw_shadows && redraw_near, draw_shadows && redraw_far, draw_shadows];
        let boxes = [(SHADOW_RANGE, light_view_proj, 0.4f32), (SHADOW_RANGE_FAR, light_view_proj_far, 6.0), (SHADOW_RANGE_CLOSE, light_view_proj_close, 0.1)];
        let dbg_shadow = f.env.debug_shadow;
        let dbg_r: f32 = f
            .env
            .debug_shadow_r
            .and_then(|v| v.trim().parse().ok())
            .unwrap_or(3.0);
        // (a copy: the casters are gathered on the worker pool, the renderer is not shared)
        let omsi_shadow_casters = self.options.omsi_shadow_casters;
        let casters = |span: std::ops::Range<usize>| -> [Vec<DrawItem>; SHADOW_LISTS] {
            let mut out: [Vec<DrawItem>; SHADOW_LISTS] = std::array::from_fn(|_| Vec::new());
            let mut ranges: Vec<(u8, u32, u32, usize, u32)> = Vec::new();
            for inst in &scene.instances[span] {
                if !inst.visible || !inst.casts_shadow || (omsi_shadow_casters && !inst.omsi_caster) {
                    continue;
                }
                // a stand-in for far tiles (`set_near_only`) left out of the picture casts no
                // shadow either: London's bridge lamps, hidden as part of a backdrop, still
                // threw their shadows on the deck (#1545)
                if let Some([x0, y0, x1, y1]) = inst.near_only {
                    let c = camera.position;
                    if c.x < x0 || c.x > x1 || c.y < y0 || c.y > y1 {
                        continue;
                    }
                }
                let m = &scene.meshes[inst.mesh];
                if m.ranges.is_empty() {
                    continue;
                }
                let (c, r) = Self::bounding_sphere(scene, inst);
                if inst.lod.0 > 0.0 || inst.lod.1 < f32::MAX {
                    let size = lod_size(inst);
                    if size < inst.lod.0 || (inst.lod.1 < f32::MAX && size >= inst.lod.1) {
                        if dbg_shadow && r >= dbg_r {
                            log::info!("shadow: mesh r={r:.1} at {:?} is another LOD than the one shown", inst.origin);
                        }
                        continue;
                    }
                }
                ranges.clear();
                for (ri, (_, _, slot)) in m.ranges.iter().enumerate() {
                    let mat_id = inst.materials.get(*slot as usize).copied().unwrap_or(0);
                    let mat = &scene.materials[mat_id];
                    let mut kind = kind_of(mat.alpha);
                    let cut_body = kind == PIPE_BLEND && mat.transmap.is_some() && !mat.no_z_write;
                    if (kind == PIPE_BLEND && !cut_body) || mat.no_z_check {
                        if dbg_shadow && r >= dbg_r {
                            log::info!("shadow: mesh r={r:.1} at {:?} slot {slot} is blended, never a caster", inst.origin);
                        }
                        continue;
                    }
                    if cut_body {
                        kind = PIPE_ALPHA_TEST;
                    }
                    // (Enhanced+: what is solid casts its shadow by the traced rays; the
                    // close and near maps keep the cut-out leaves and fences, whose texels
                    // the rays cannot see)
                    if rt_frame && kind == PIPE_OPAQUE && !moon_shadows {
                        kind = PIPE_KINDS;
                    }
                    ranges.push((kind, ri as u32, *slot, mat_id, mat.look));
                }
                if !lamp_shadows.is_empty() && !(m.bounds_radius > 0.0 && m.bounds_radius < 0.1) && lamp_reach(c, r) {
                    // (each lamp's map takes the casters within its own reach: one list
                    // drawn into every map cost each lamp the others' casters as well)
                    for (k, light) in lamp_shadows.iter().enumerate() {
                        if (light.owner.is_some() && light.owner == inst.shadow_owner) || !lamp_reaches(light, c, r) {
                            continue;
                        }
                        for &(kind, ri, slot, mat_id, look) in &ranges {
                            let kind = if kind == PIPE_KINDS { PIPE_OPAQUE } else { kind };
                            let (material, look) = depth_only_material(kind, mat_id, look);
                            out[3 + k].push(DrawItem { pipe: kind, mesh: inst.mesh as u32, range: ri, material, look, entry: inst.base + slot });
                        }
                    }
                }
                for (cascade, &(range, lvp, min_radius)) in boxes.iter().enumerate() {
                    if !active[cascade] {
                        continue;
                    }
                    let dbg = dbg_shadow && cascade == 0 && r >= dbg_r;
                    if m.bounds_radius > 0.0 && m.bounds_radius < min_radius {
                        if dbg_shadow && cascade == 0 && m.bounds_radius >= dbg_r {
                            log::info!("shadow: mesh r={:.1} skipped (ranges {})", m.bounds_radius, m.ranges.len());
                        }
                        continue;
                    }
                    let lc = lvp.project_point3(c);
                    let rr = r / range;
                    if lc.x.abs() > 1.0 + rr || lc.y.abs() > 1.0 + rr {
                        if dbg {
                            log::info!("shadow: mesh r={r:.1} outside the light box at ({:.2}, {:.2})", lc.x, lc.y);
                        }
                        continue;
                    }
                    if dbg {
                        log::info!("shadow: caster r={r:.1} at {:?} slots {:?}", inst.origin, ranges.iter().map(|x| x.0).collect::<Vec<_>>());
                    }
                    for &(kind, ri, slot, mat_id, look) in &ranges {
                        let kind = if kind == PIPE_KINDS {
                            if cascade != 1 {
                                continue;
                            }
                            PIPE_OPAQUE
                        } else {
                            kind
                        };
                        // (after the remap: a traced-opaque one is drawn as the opaque it is)
                        let (material, look) = depth_only_material(kind, mat_id, look);
                        out[cascade].push(DrawItem {
                            pipe: kind,
                            mesh: inst.mesh as u32,
                            range: ri,
                            material,
                            look,
                            entry: inst.base + slot,
                        });
                    }
                }
            }
            out
        };
        if active.iter().any(|a| *a) || !lamp_shadows.is_empty() {
            let n = scene.instances.len();
            let parts = (n / 8192).clamp(1, self.encoding_pool.as_ref().map_or(3, |p| p.current_num_threads()) + 1);
            let chunk = n.div_ceil(parts).div_ceil(CULL_BLOCK) * CULL_BLOCK;
            let blocks = Self::cull_blocks(scene);
            let lit = |b: usize| {
                blocks.as_ref().is_none_or(|bl| {
                    let (c, r) = bl[b];
                    lamp_reach(c, r) || boxes.iter().enumerate().any(|(k, &(range, lvp, _))| {
                        let lc = lvp.project_point3(c);
                        let rr = r / range;
                        active[k] && lc.x.abs() <= 1.0 + rr && lc.y.abs() <= 1.0 + rr
                    })
                })
            };
            let mut found: [Vec<DrawItem>; SHADOW_LISTS] = std::array::from_fn(|_| Vec::new());
            for part in run_parts(self.encoding_pool.as_ref(), parts, |p| {
                let mut out: [Vec<DrawItem>; SHADOW_LISTS] = std::array::from_fn(|_| Vec::new());
                let end = ((p + 1) * chunk).min(n);
                let mut b = p * chunk;
                while b < end {
                    let next = (b + CULL_BLOCK).min(end);
                    if lit(b / CULL_BLOCK) {
                        for (a, x) in out.iter_mut().zip(casters(b..next)) {
                            a.extend(x);
                        }
                    }
                    b = next;
                }
                out
            }) {
                for (a, b) in found.iter_mut().zip(part) {
                    a.extend(b);
                }
            }
            for cascade in 0..SHADOW_LISTS {
                if cascade < 3 && !active[cascade] {
                    continue;
                }
                if debug_draws {
                    log::info!("shadow cascade {cascade}: {} draws", found[cascade].len());
                }
                batch_items(scene, &mut found[cascade], true, list, &mut shadow_batches[cascade]);
            }
        }
        if sh.shadows && debug_draws {
            log::info!(
                "shadow passes: {} batches",
                shadow_batches.iter().map(|b| b.len()).sum::<usize>()
            );
        }
        shadow_batches
    }
}
