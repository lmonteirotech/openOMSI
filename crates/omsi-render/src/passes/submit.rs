//! The frame's command buffers finished (side by side when there is much to finish) and
//! submitted.

use super::*;

impl Renderer {
    /// Finish the encoders and submit them: the shadow maps, the prepass, the main pass's
    /// parts, then the picture. `big`: finished on helper threads.
    pub(crate) fn finish_and_submit(&mut self, encoders: Encoders, main_parts: Vec<wgpu::CommandEncoder>, big: bool, with_overlays: bool, clock: &mut StageClock, timers: PassTimers) {
        let Encoders { shadow: shadow_encoder, prepass: prepass_encoder, picture: encoder } = encoders;
        let profiling = self.profiling;
        let finish = |encoder: wgpu::CommandEncoder| {
            let start = profiling.then(std::time::Instant::now);
            let commands = encoder.finish();
            (commands, start.map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0))
        };
        let (shadow_commands, prepass_commands, part_commands, commands, finish_times) = if big && self.encoding_pool.is_some() {
            self.encoding_pool.as_ref().unwrap().in_place_scope_fifo(|scope| {
                let (shadow_tx, shadow_rx) = std::sync::mpsc::sync_channel(1);
                let (prepass_tx, prepass_rx) = std::sync::mpsc::sync_channel(1);
                let parts = main_parts.len();
                let (part_tx, part_rx) = std::sync::mpsc::sync_channel(parts.max(1));
                let finish = &finish;
                scope.spawn_fifo(move |_| { let _ = shadow_tx.send(finish(shadow_encoder)); });
                scope.spawn_fifo(move |_| { let _ = prepass_tx.send(finish(prepass_encoder)); });
                for (k, e) in main_parts.into_iter().enumerate() {
                    let tx = part_tx.clone();
                    scope.spawn_fifo(move |_| { let _ = tx.send((k, finish(e).0)); });
                }
                let (commands, main_secs) = finish(encoder);
                let wait = std::time::Instant::now();
                let (shadow_commands, shadow_secs) = shadow_rx.recv().expect("command encoding worker");
                let shadow_wait = wait.elapsed().as_secs_f64();
                let wait = std::time::Instant::now();
                let (prepass_commands, prepass_secs) = prepass_rx.recv().expect("command encoding worker");
                let mut part_commands: Vec<Option<wgpu::CommandBuffer>> = (0..parts).map(|_| None).collect();
                for _ in 0..parts {
                    let (k, c) = part_rx.recv().expect("command encoding worker");
                    part_commands[k] = Some(c);
                }
                (
                    shadow_commands,
                    prepass_commands,
                    part_commands.into_iter().map(|c| c.expect("main pass part")).collect::<Vec<_>>(),
                    commands,
                    [shadow_secs, prepass_secs, main_secs, shadow_wait, wait.elapsed().as_secs_f64()],
                )
            })
        } else if big {
            std::thread::scope(|scope| {
                let finish = &finish;
                let shadow = scope.spawn(move || finish(shadow_encoder));
                let prepass = scope.spawn(move || finish(prepass_encoder));
                let parts: Vec<_> = main_parts.into_iter().map(|e| scope.spawn(move || finish(e).0)).collect();
                let (commands, main_secs) = finish(encoder);
                let wait = std::time::Instant::now();
                let (shadow_commands, shadow_secs) = shadow.join().expect("command encoding thread");
                let shadow_wait = wait.elapsed().as_secs_f64();
                let wait = std::time::Instant::now();
                let (prepass_commands, prepass_secs) = prepass.join().expect("command encoding thread");
                (
                    shadow_commands,
                    prepass_commands,
                    parts.into_iter().map(|h| h.join().expect("command encoding thread")).collect::<Vec<_>>(),
                    commands,
                    [shadow_secs, prepass_secs, main_secs, shadow_wait, wait.elapsed().as_secs_f64()],
                )
            })
        } else {
            let (shadow, shadow_secs) = finish(shadow_encoder);
            let (prepass, prepass_secs) = finish(prepass_encoder);
            let parts: Vec<_> = main_parts.into_iter().map(|e| finish(e).0).collect();
            let (main, main_secs) = finish(encoder);
            (shadow, prepass, parts, main, [shadow_secs, prepass_secs, main_secs, 0.0, 0.0])
        };
        if self.profiling {
            // These overlap across helper threads; do not add them to the stage totals.
            let keys = if with_overlays {
                ["finish.shadow", "finish.prepass", "finish.main", "finish.wait shadow", "finish.wait prepass"]
            } else {
                ["mirror.finish.shadow", "mirror.finish.prepass", "mirror.finish.main", "mirror.finish.wait shadow", "mirror.finish.wait prepass"]
            };
            for (key, secs) in keys.into_iter().zip(finish_times) {
                *self.stats.borrow_mut().entry(key).or_default() += secs;
            }
        }
        clock.stage(self, "finish", "mirror.finish");
        self.queue
            .submit([shadow_commands, prepass_commands].into_iter().chain(part_commands).chain([commands]));
        clock.stage(self, "submit", "mirror.submit");
        let timed = timers.timed;
        if let (Some(t), false) = (
            self.gpu_timers[with_overlays as usize].as_mut(),
            timed.is_empty(),
        ) {
            t.pending = timed;
            t.unresolved = true;
        }
    }
}
