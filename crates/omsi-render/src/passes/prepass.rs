//! The passes of the prepass command buffer: the depth prepass with the ambient
//! occlusion, and the enhanced path's sky cube and reflection probe.

use super::*;

impl Renderer {
    // --- depth prepass + ambient occlusion (single-sampled, camera projection)
    pub(crate) fn encode_depth_prepass(&self, prepass_encoder: &mut wgpu::CommandEncoder, scene: &Scene, f: &FrameCtx, prepass_batches: &[Batch], timers: &mut PassTimers) {
        let (camera, aspect, width, height, ao_on) = (f.camera, f.aspect, f.width, f.height, f.ao_on);
        if f.prepass_on {
            let proj = f.projection.unwrap_or_else(|| {
                Mat4::perspective_rh(camera.fov_deg.to_radians(), aspect, camera.far, camera.near)
            });
            let u = SsaoUniform {
                inv_proj: proj.inverse().to_cols_array_2d(),
                params: [
                    1.0,
                    1.4,
                    width.div_ceil(2) as f32,
                    height.div_ceil(2) as f32,
                ],
                shift: [proj.z_axis.x, proj.z_axis.y, 0.0, 0.0],
            };
            self.queue
                .write_buffer(&self.ao_buf, 0, bytemuck::bytes_of(&u));
            let ao = self.ao.as_ref().unwrap();
            {
                let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("depth prepass"),
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &ao.depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(0.0),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: pass_timer(timers.set.as_ref(), &mut timers.timed, "depth prepass"),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
                encode_batches(&mut pass, scene, prepass_batches, |pipe| {
                    &self.prepass_pipelines[pipe as usize]
                });
            }
            for (pipe, bg, target, pass_label) in [
                (&self.ssao_pipeline, &ao.ssao_bg, &ao.ao_view, "ssao"),
                (&self.blur_pipeline, &ao.blur_bg, &ao.blur_view, "ssao blur"),
            ] {
                let Some(pipe) = pipe.as_ref().filter(|_| ao_on) else {
                    break;
                };
                let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("ssao"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: target,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: pass_timer(timers.set.as_ref(), &mut timers.timed, pass_label),
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_pipeline(pipe);
                pass.set_bind_group(0, bg, &[]);
                pass.draw(0..3, 0..1);
            }
        }
    }

    // --- the enhanced sky cube: a face a frame (all six the first time and for a new
    // sky), drawn in the window's frame only
    pub(crate) fn encode_sky_cube(&mut self, prepass_encoder: &mut wgpu::CommandEncoder, scene: &Scene, f: &FrameCtx, redraw_near: bool, probe_redraw: bool, timers: &mut PassTimers) {
        let (enhanced, lead_view) = (f.enhanced, f.lead_view);
        if enhanced && (lead_view || probe_redraw) {
            if let (Some(probe), Some(sky_bg)) = (self.probe.as_mut(), scene.sky_bind_group.as_ref()) {
                // (face, round, the old picture's share): a whole new cube is every round
                // of every face averaged; afterwards one face a frame, blended in
                let full = !probe.cube_filled || self.instant_exposure;
                probe.cube_wait += 1;
                let recapture = std::mem::take(&mut probe.cube_recapture);
                let draws: Vec<(u32, u32, f64)> = if full {
                    (0..6)
                        .flat_map(|f| (0..SKY_CUBE_ROUNDS).map(move |r| (f, r, r as f64 / (r as f64 + 1.0))))
                        .collect()
                } else if recapture {
                    // the eye moved on: every face from the new one, nothing kept from the
                    // old place
                    let round = (probe.cube_round / 6) % SKY_CUBE_ROUNDS;
                    (0..6).map(|f| (f, round, 0.0)).collect()
                } else if (probe.cube_wait >= SKY_CUBE_EVERY && !redraw_near) || probe.cube_wait >= SKY_CUBE_EVERY * 2 || !lead_view {
                    // (on a frame that keeps the near shadow map: the two costliest
                    // occasional passes never fall on the same frame)
                    vec![(probe.cube_next, (probe.cube_round / 6) % SKY_CUBE_ROUNDS, SKY_CUBE_HISTORY)]
                } else {
                    Vec::new()
                };
                let single = draws.len() == 1;
                if !draws.is_empty() {
                    probe.cube_wait = 0;
                    probe.cube_next = (probe.cube_next + 1) % 6;
                    probe.cube_round = probe.cube_round.wrapping_add(1);
                }
                probe.cube_filled = true;
                for (f, round, history) in draws {
                    let mut pass = prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                        label: Some("sky cube"),
                        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                            view: &probe.cube_faces[f as usize],
                            depth_slice: None,
                            resolve_target: None,
                            ops: wgpu::Operations {
                                load: if history > 0.0 { wgpu::LoadOp::Load } else { wgpu::LoadOp::Clear(wgpu::Color::BLACK) },
                                store: wgpu::StoreOp::Store,
                            },
                        })],
                        depth_stencil_attachment: None,
                        timestamp_writes: if single { pass_timer(timers.set.as_ref(), &mut timers.timed, "sky cube") } else { None },
                        occlusion_query_set: None,
                        multiview_mask: None,
                    });
                    pass.set_pipeline(&probe.cube_pipeline);
                    pass.set_blend_constant(wgpu::Color { r: history, g: history, b: history, a: history });
                    pass.set_bind_group(0, &probe.cube_bind_groups[(f * SKY_CUBE_ROUNDS + round) as usize], &[]);
                    pass.set_bind_group(1, sky_bg, &[]);
                    pass.draw(0..3, 0..1);
                }
            }
        }
    }

    // --- the reflection probe of the enhanced path: the sky into the six faces, then
    // each blurrier level from the sharper ones (three faces a pass)
    pub(crate) fn encode_probe(&self, prepass_encoder: &mut wgpu::CommandEncoder, scene: &Scene, probe_redraw: bool, timers: &mut PassTimers) {
        if probe_redraw {
            if let (Some(probe), Some(sky_bg)) =
                (self.probe.as_ref(), scene.sky_bind_group.as_ref())
            {
                for (m, faces) in probe.faces.iter().enumerate() {
                    for (half, bg) in probe.bind_groups[m].iter().enumerate() {
                        let attachments: Vec<Option<wgpu::RenderPassColorAttachment>> = faces
                            [half * 3..half * 3 + 3]
                            .iter()
                            .map(|v| {
                                Some(wgpu::RenderPassColorAttachment {
                                    view: v,
                                    depth_slice: None,
                                    resolve_target: None,
                                    ops: wgpu::Operations {
                                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                                        store: wgpu::StoreOp::Store,
                                    },
                                })
                            })
                            .collect();
                        let mut pass =
                            prepass_encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                                label: Some("reflection probe"),
                                color_attachments: &attachments,
                                depth_stencil_attachment: None,
                                timestamp_writes: if m == 0 && half == 0 {
                                    pass_timer(timers.set.as_ref(), &mut timers.timed, "probe")
                                } else {
                                    None
                                },
                                occlusion_query_set: None,
                                multiview_mask: None,
                            });
                        pass.set_pipeline(if m == 0 {
                            &probe.sky_pipeline
                        } else {
                            &probe.filter_pipeline
                        });
                        pass.set_bind_group(0, bg, &[]);
                        pass.set_bind_group(1, sky_bg, &[]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
    }
}
