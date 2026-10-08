//! Discord, Steam, the Lua plugins and the personnel file's counts in the window's frame.

use super::*;

impl App {
    /// Discord's status, Steam's callbacks, the plugins' frame (and what they asked the game
    /// to do), `OMSI_WATCH_VARS`, and the people's counts for the personnel file.
    pub(super) fn frame_integrations(&mut self, event_loop: &ActiveEventLoop, dt: f32) {
        // Discord's status: the map, the bus, the line (every few seconds)
        #[cfg(not(target_os = "android"))]
        {
            self.integrations.discord_t -= dt;
            if self.integrations.discord_t <= 0.0 {
                self.integrations.discord_t = 5.0;
                if self.args.server.is_none()
                    && self.integrations.discord.is_none()
                    && self.settings.discord_status
                {
                    self.integrations.discord =
                        crate::discord::Discord::start(&self.settings.discord_app_id);
                }
                if let Some(d) = self.integrations.discord.as_ref() {
                    let bus = self.player.as_ref().map(|p| {
                        let definition = &p.vehicle.ty.def;
                        let short = omsi_launcher_lib::vehicle_type_label(&definition.type_name, &definition.path);
                        let full = omsi_launcher_lib::display_bus_name(&format!("{} {short}", definition.manufacturer));
                        (short, full)
                    });
                    let duty = self.session.duty.as_ref().map(|d| (d.line.as_str(), d.tour.as_str()));
                    d.set(crate::discord::Presence::for_game(
                        self.world.as_ref().map(|w| w.global.name.as_str()),
                        bus.as_ref().map(|(short, full)| (short.as_str(), full.as_str())),
                        duty,
                        self.net.lan.is_some(),
                    ));
                }
            }
        }

        // Steam's callbacks (rich presence)
        #[cfg(steam)]
        if let Some(steam) = self.integrations.steam.as_ref() {
            steam.client.run_callbacks();
        }
        // the plugins' frame, with the bus's scripts done
        let plugins = self.integrations.plugins.get_or_insert_with(crate::plugins::load);
        if !plugins.is_empty() && !self.paused {
            let info = crate::plugins::game_info(self);
            let keys = std::mem::take(&mut self.integrations.plugin_keys);
            let events = std::mem::take(&mut self.integrations.plugin_events);
            let plugins = self.integrations.plugins.as_mut().unwrap();
            // the vehicles around it: the AI traffic and the other players' buses
            let mut others: Vec<(u64, &'static str, &mut omsi_sim::VehicleInstance)> = Vec::new();
            if let Some(t) = self.session.traffic.as_mut() {
                others.extend(t.cars.iter_mut().map(|c| (c.id, "ai", &mut c.vehicle)));
            }
            others.extend(self.net.remotes.remotes.iter_mut().map(|(id, r)| ((1u64 << 48) | *id as u64, "player", r.vehicle_mut())));
            let mut io = crate::plugins::Io { vehicle: self.player.as_mut().map(|p| &mut p.vehicle), others, dt, message: None, info, commands: Vec::new(), keys, events };
            plugins.frame(&mut io);
            let commands = std::mem::take(&mut io.commands);
            if let Some(m) = io.message {
                self.service_msg = Some(m);
            }
            // what the plugins asked the game to do: lines of the game menu
            self.integrations.plugin_command = true;
            for c in commands {
                if let Some(k) = self.game_menu_items().iter().position(|m| m.0 == c) {
                    let was = self.menus.game_menu;
                    self.menus.menu_prev_pause = self.paused;
                    self.menu_choose(event_loop, k);
                    // (an action leaves the menu as it found it)
                    if self.menus.chooser.is_none() && was.is_none() {
                        self.menus.game_menu = None;
                    }
                } else {
                    // (a line of the vehicle or world pages)
                    self.menus.menu_prev_pause = self.paused;
                    self.page_action(&c);
                }
            }
            self.integrations.plugin_command = false;
        } else {
            self.integrations.plugin_keys.clear();
            // (while the game is paused they wait for the next frame)
            if self.integrations.plugins.as_ref().is_none_or(|p| p.is_empty()) {
                self.integrations.plugin_events.clear();
            }
        }
        // OMSI_WATCH_VARS=a,b: every change of those variables of the player's bus
        if let (Some(p), Some(list)) = (self.player.as_ref(), omsi_cfg::flags::OMSI_WATCH_VARS.var()) {
            thread_local!(static LAST: std::cell::RefCell<std::collections::HashMap<String, f32>> = Default::default());
            LAST.with(|last| {
                let mut last = last.borrow_mut();
                for n in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    let v = p.vehicle.var(n).unwrap_or(f32::NAN);
                    if last.get(n).is_none_or(|&o| o.to_bits() != v.to_bits()) {
                        log::info!("watch: {n} = {v} at {:.2} s", self.clock.time);
                        last.insert(n.to_string(), v);
                    }
                }
            });
        }
        if let (Some(h), Some(p)) = (self.session.humans.as_mut(), self.player.as_ref()) {
            let hurt = steps::people_in_career(&mut self.session.career, h, p, self.settings.collision_pedestrians);
            for (name, price) in h.take_sales() {
                let args = vec![omsi_plugin::InfoValue::Text(name.trim().to_string()), crate::plugins::num_f32(price)];
                crate::plugins::queue_event(&mut self.integrations.plugin_events, "ticket_sold", args);
            }
            if hurt > 0 {
                self.service_msg = Some(("Pedestrian knocked down!".into(), 6.0));
                crate::plugins::queue_event(&mut self.integrations.plugin_events, "pedestrian", vec![omsi_plugin::InfoValue::Num(hurt as f64)]);
            }
        }
    }
}
