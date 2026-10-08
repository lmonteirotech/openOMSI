//! Timetable buses as traffic: omsi-sim's `ai_traffic::bus_service`, here by its old path
//! (the timetable and the LAN code name it so).

pub(crate) use omsi_sim::ai_traffic::bus_service::*;

#[cfg(test)]
mod tests {
    use super::*;
    use omsi_sim::VehicleInstance;

    #[test]
    fn scheduled_ai_exposes_timetable_during_init_and_frame_ai() {
        let template = crate::schedule::tests::script_test_vehicle(
            "{init}\n(L.L.schedule_active) (S.L.init_active)\n(M.V.GetTTBusstopCount) (S.L.init_count)\n(M.V.GetTTLineString) (S.$.init_line)\n{end}\n{frame_ai}\n(L.L.schedule_active) (S.L.frame_active)\n(M.V.GetTTBusstopIndex) (S.L.frame_index)\n{end}\n",
            "schedule_active\ninit_active\ninit_count\nframe_active\nframe_index\n",
            "init_line\n",
        );
        let stop = Stop::from_tuple((0, 10.0, 0.0, 120.0, 7, 0.0));
        let timetable = AiTimetable {
            line: "042".into(),
            terminus: "terminus".into(),
            stops: vec![
                (7, "First".into(), 100.0, 120.0),
                (8, "Last".into(), 200.0, 220.0),
            ],
        };
        let mut host = omsi_sim::VehicleHost::new(Default::default());
        timetable.install(&mut host, Some(&stop));
        let mut vehicle = VehicleInstance::new(template.ty.clone(), host);
        assert_eq!(vehicle.var("init_active"), Some(1.0));
        assert_eq!(vehicle.var("init_count"), Some(2.0));
        assert_eq!(vehicle.str_var("init_line"), "042");
        vehicle.set_var("schedule_active", 0.0);
        vehicle.update_ai(0.01, &Default::default());
        assert_eq!(vehicle.var("frame_active"), Some(1.0));
        assert_eq!(vehicle.var("frame_index"), Some(0.0));
        // Ordinary traffic has no assigned timetable; it must remain inactive.
        let mut ordinary = VehicleInstance::new(
            template.ty.clone(),
            omsi_sim::VehicleHost::new(Default::default()),
        );
        ordinary.update_ai(0.01, &Default::default());
        assert_eq!(ordinary.var("init_active"), Some(0.0));
        assert_eq!(ordinary.var("frame_active"), Some(0.0));
    }

    #[test]
    fn timetable_tracks_repeated_stops_streaming_and_trip_changes() {
        let stop = |id, depart| Stop::from_tuple((0, 10.0, 0.0, depart, id, 0.0));
        let first = stop(7, 120.0);
        let second = stop(8, 220.0);
        let repeated = stop(7, 320.0);
        let timetable = AiTimetable {
            line: "42".into(),
            terminus: "terminal".into(),
            stops: vec![
                (7, "A".into(), 100.0, 120.0),
                (8, "B".into(), 200.0, 220.0),
                (7, "A".into(), 300.0, 320.0),
            ],
        };
        let mut v =
            crate::schedule::tests::script_test_vehicle("{frame_ai}\n{end}\n", "schedule_active\n", "");
        v.host.hof = Some(std::sync::Arc::new(omsi_vehicle::Hof {
            termini: vec![omsi_vehicle::hof::Terminus {
                texture_id: "terminal".into(),
                ..Default::default()
            }],
            ..Default::default()
        }));
        timetable.install(&mut v.host, Some(&first));
        assert_eq!(v.host.tt_terminus_index, 0);
        let mut service = BusService::new(vec![first, second, repeated]);
        service.phase = Phase::Boarding;
        service.feed_timetable(&mut v, 110.0);
        assert_eq!(v.host.tt_delay, -10.0);
        service.stops.pop_front();
        service.phase = Phase::Running;
        service.delay = 5.0;
        service.feed_timetable(&mut v, 210.0);
        assert_eq!(v.host.tt_busstop_index, 1);
        assert_eq!(v.host.tt_delay, 10.0);
        service.stops.pop_front();
        service.feed_timetable(&mut v, 310.0);
        assert_eq!(v.host.tt_busstop_index, 2);
        // Exhausting the loaded geometry does not exhaust the script's timetable.
        service.stops.clear();
        service.route_open = true;
        service.feed_timetable(&mut v, 315.0);
        assert_eq!(v.host.tt_stops.len(), 3);
        assert_eq!(v.host.tt_busstop_index, 2);
        assert_eq!(v.host.schedule_active, 1.0);
        let next = AiTimetable {
            line: "43".into(),
            terminus: "missing".into(),
            stops: vec![(9, "C".into(), 400.0, 420.0)],
        };
        next.install(&mut v.host, Some(&stop(9, 420.0)));
        assert_eq!(v.host.tt_line, "43");
        assert_eq!(v.host.tt_stops.len(), 1);
        assert_eq!(v.host.tt_busstop_index, 0);
        assert_eq!(v.host.tt_terminus_index, -1);
        assert_eq!(v.host.tt_delay, 0.0);
    }
}
