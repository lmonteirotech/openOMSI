//! The passes over the finished main picture: the reflections (traced or the puddles'),
//! the lamps' light in the fog, the rain films on the panes and the classic graphics'
//! reflections presented.

use super::*;

impl Renderer {
    /// The traced reflections (Enhanced+) or the puddles'; true when the puddles' picture
    /// was drawn this frame.
    pub(crate) fn encode_scene_reflections(&mut self, encoder: &mut wgpu::CommandEncoder, scene: &Scene, f: &FrameCtx, plan: &DrawPlan, cu: &CameraUniform, timers: &mut PassTimers) -> bool {
        let (width, height, camera, lighting, rt_frame) = (f.width, f.height, f.camera, f.lighting, f.rt_frame);
        let (main_batches, cab_batches) = (&plan.main_batches, &plan.cab_batches);
        // Weather alone is insufficient: leave the allocation and reflection passes out when
        // the visible batches contain no moisture-tagged surface (a showroom, bare terrain).
        // Enhanced+: the traced reflections (wet roads' too) in place of the puddles' rays
        if rt_frame {
            self.encode_rt_reflections(encoder, width, height, timers.set.as_ref(), &mut timers.timed);
        }
        let puddles_on = f.puddles_wanted
            && !rt_frame
            && main_batches.iter().any(|b| scene.materials[b.material as usize].uniform.params2[2] > 0.0)
            && self.prepare_puddle_reflections(width, height, camera, f.aspect, f.projection, cu, lighting, f.ro);
        if puddles_on {
            // (the vehicle the camera is in is drawn into the puddles' picture as well)
            let all: Vec<Batch>;
            let batches = if cab_batches.is_empty() {
                main_batches
            } else {
                all = main_batches.iter().chain(cab_batches).cloned().collect();
                &all
            };
            self.encode_puddle_reflections(encoder, width, height, scene, batches, &plan.list, lighting, camera, timers.set.as_ref(), &mut timers.timed);
        }
        puddles_on
    }

    // the lamps' light in the weather's fog and the sun's shafts between the shadows
    // (before the rain on the panes, which writes the prepass depth),
    // over the picture before its glow and metering (the window's picture; the mirrors
    // go without)
    pub(crate) fn encode_fog_lamps(&self, encoder: &mut wgpu::CommandEncoder, scene: &Scene, f: &FrameCtx, vp_mat: Mat4, puddles_on: bool, timers: &mut PassTimers) {
        let (width, height, lighting) = (f.width, f.height, f.lighting);
        let (enhanced, prepass_on, with_overlays, masked_frame) = (f.enhanced, f.prepass_on, f.with_overlays, f.masked_frame);
        let fog_lamps = enhanced_weather_fog(lighting) >= 2e-4 && !scene.lights.is_empty();
        let shafts = lighting.shadows && lighting.sun_dir.z > 0.0;
        let glare = lighting.sun_dir.z > -0.01 && lighting.sun_intensity > 0.0 && !f.env.no_glare;
        if f.env.debug_fog_lamps {
            log::info!("fog lamps: enhanced {enhanced} prepass {prepass_on} overlays {with_overlays} masked {masked_frame} fog {:.5} lights {} shafts {shafts} glare {glare}", enhanced_weather_fog(lighting), scene.lights.len());
        }
        if enhanced && prepass_on && with_overlays && masked_frame && (fog_lamps || shafts || glare) {
            if let (Some(pipes), Some(ao), Some(cam_bg)) = (self.fog_lamps_pipeline.as_ref(), self.ao.as_ref(), scene.camera_bind_group.as_ref()) {
                if let Some(fog_bg) = ao.fog_bg.as_ref() {
                    // (how much of the weather's extinction is mist and fog - droplets of some
                    // ten micrometres, which scatter a lamp's light into a halo and its beam
                    // into a cone - rather than rain: a raindrop of a millimetre sends what it
                    // scatters on within a few hundredths of a degree, no halo round a lamp,
                    // no cone of a headlight in a drizzle (the rain's share as lights.rs
                    // `apply_weather` lays it on))
                    let fog_all = enhanced_weather_fog(lighting);
                    let rain_part = if lighting.rain > 0.0 && lighting.snowfall <= 0.0 { 2.3 / (2500.0 - 1800.0 * lighting.rain.clamp(0.0, 1.0)) } else { 0.0 };
                    let droplets = if fog_all > 0.0 { ((fog_all - rain_part) / fog_all).clamp(0.0, 1.0) } else { 0.0 };
                    let u = FogLampUniform { inv_view_proj: vp_mat.inverse().to_cols_array_2d(), size: [width as f32, height as f32, if glare { 1.0 } else { 0.0 }, droplets] };
                    self.queue.write_buffer(&self.fog_lamps_buf, 0, bytemuck::bytes_of(&u));
                    let h = &self.hdr_targets[&(width, height)];
                    let view = h.puddles.as_ref().filter(|_| puddles_on).map_or(&h.view, |p| &p.view);
                    for (k, (target, load)) in [(&ao.fog_view, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT)), (view, wgpu::LoadOp::Load)].into_iter().enumerate() {
                        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                            label: Some("fog lamps"),
                            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                                view: target, depth_slice: None, resolve_target: None,
                                ops: wgpu::Operations { load, store: wgpu::StoreOp::Store },
                            })],
                            depth_stencil_attachment: None,
                            timestamp_writes: if k == 1 { pass_timer(timers.set.as_ref(), &mut timers.timed, "fog lamps") } else { None },
                            occlusion_query_set: None,
                            multiview_mask: None,
                        });
                        pass.set_pipeline(&pipes[k]);
                        pass.set_bind_group(0, cam_bg, &[]);
                        pass.set_bind_group(1, &fog_bg[k], &[]);
                        pass.draw(0..3, 0..1);
                    }
                }
            }
        }
    }

    /// The rain films on the panes, over a copy of the clean current picture.
    pub(crate) fn encode_rain_film(&self, encoder: &mut wgpu::CommandEncoder, scene: &Scene, f: &FrameCtx, rain_batches: &[Batch], puddles_on: bool) {
        let (width, height, masked_frame) = (f.width, f.height, f.masked_frame);
        let scene_view = f.scene_view();
        if f.glass_on {
            let hdr = masked_frame.then(|| &self.hdr_targets[&(width, height)]);
            let view = hdr.map_or(scene_view, |h| h.puddles.as_ref().filter(|_| puddles_on).map_or(&h.view, |p| &p.view));
            let behind = self.glass_picture.as_ref().unwrap();
            encoder.copy_texture_to_texture(
                wgpu::TexelCopyTextureInfo { texture: view.texture(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::TexelCopyTextureInfo { texture: behind.texture(), mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            );
            let colours = [
                Some(wgpu::RenderPassColorAttachment {
                    view, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                }),
                hdr.map(|h| wgpu::RenderPassColorAttachment {
                    view: &h.mask, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                }),
                hdr.and_then(|h| h.gbuf.as_ref()).map(|g| wgpu::RenderPassColorAttachment {
                    view: &g[0].1, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                }),
                hdr.and_then(|h| h.gbuf.as_ref()).map(|g| wgpu::RenderPassColorAttachment {
                    view: &g[1].1, depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store },
                }),
            ];
            let pipes = &self.main_pass(f.enhanced, f.reflection_frame).rain_pipelines;
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("rain on current scene"),
                color_attachments: &colours[..if colours[2].is_some() { 4 } else if hdr.is_some() { 2 } else { 1 }],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &self.ao.as_ref().unwrap().depth_view,
                    depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Load, store: wgpu::StoreOp::Store }),
                    stencil_ops: None,
                }),
                timestamp_writes: None, occlusion_query_set: None, multiview_mask: None,
            });
            pass.set_bind_group(0, scene.camera_bind_group.as_ref().unwrap(), &[]);
            encode_batches(&mut pass, scene, rain_batches, |pipe| &pipes[pipe as usize]);
        }
    }

    /// The classic graphics' picture with the puddles' reflections, presented (and the
    /// HUD over it at full size).
    pub(crate) fn encode_classic_present(&self, encoder: &mut wgpu::CommandEncoder, scene: &Scene, f: &FrameCtx, puddles_on: bool) {
        let (width, height) = (f.width, f.height);
        let overlays = &f.overlays;
        if f.reflection_frame {
            let h = &self.hdr_targets[&(width, height)];
            let bg = h.puddles.as_ref().filter(|_| puddles_on).map_or(&h.classic_bg, |p| &p.classic_bg);
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("classic reflections present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: f.scene_view(), depth_slice: None, resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::BLACK), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None, timestamp_writes: None,
                occlusion_query_set: None, multiview_mask: None,
            });
            pass.set_pipeline(&self.copy_pipeline);
            pass.set_bind_group(0, bg, &[]);
            pass.draw(0..3, 0..1);
            if !overlays.is_empty() && !f.scaled {
                pass.set_pipeline(&self.overlay_pipeline_1x);
                for (k, _) in overlays.iter().enumerate() {
                    if let Some((_, _, bg, _)) = scene.overlay_res.get(k) {
                        pass.set_bind_group(0, bg, &[]);
                        pass.draw(0..6, 0..1);
                    }
                }
            }
        }
    }
}
