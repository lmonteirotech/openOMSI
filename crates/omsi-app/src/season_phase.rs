//! A season chosen with its phase - early, mid or late - instead of a date.
//!
//! The choice stands for a real time of the year: the meteorological season's first,
//! second or third month (half a year later south of the equator), on a day of it that is
//! typical of the phase. Everything that goes by the date follows from that by itself:
//! the sun's path and the day's length, the physical weather's temperature, humidity and
//! snow, the street lamps, the map's chrono changes, the timetable's day types.
//!
//! What the map looks like comes from its texture seasons (`[addseason]` folders): a phase
//! lies between two of them - early autumn between summer and `Fall`, late autumn between
//! `Fall` and the bare `Winter` - and the plants are mixed between the two looks (see
//! `omsi_texture::season_mix`), while everything else shows the look that prevails.

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SeasonName {
    Spring,
    Summer,
    Autumn,
    Winter,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    Early,
    Mid,
    Late,
}

/// A season and its phase (`--season autumn-late`; `autumn` alone is its middle).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct SeasonChoice {
    pub season: SeasonName,
    pub phase: Phase,
}

/// Texture season kinds of the map's `[addseason]` (see `omsi_map::global::Season::folder`).
const SUMMER: i32 = 0;
const SPRING: i32 = 1;
const FALL: i32 = 2;
const WINTER: i32 = 3;
const SUMMER_DRY: i32 = 5;

impl SeasonChoice {
    /// `spring`, `autumn-late`, `herbst früh`, `winter_early`… (None: no season, by date).
    pub(crate) fn parse(s: &str) -> Option<SeasonChoice> {
        let s = s.trim().to_lowercase();
        let mut words = s.split(['-', '_', ' ', '/']).filter(|w| !w.is_empty());
        let season = match words.next()? {
            "spring" | "fruehling" | "frühling" => SeasonName::Spring,
            "summer" | "sommer" => SeasonName::Summer,
            "autumn" | "fall" | "herbst" => SeasonName::Autumn,
            "winter" => SeasonName::Winter,
            _ => return None,
        };
        let phase = match words.next() {
            None | Some("mid" | "middle" | "mitte") => Phase::Mid,
            Some("early" | "frueh" | "früh") => Phase::Early,
            Some("late" | "spaet" | "spät") => Phase::Late,
            Some(_) => return None,
        };
        Some(SeasonChoice { season, phase })
    }

    /// The choice as the command line and the LAN world write it: the season alone for
    /// its middle (what older versions read), else `season-phase`.
    pub(crate) fn word(&self) -> String {
        let season = self.season_word();
        match self.phase {
            Phase::Mid => season.to_string(),
            p => format!("{season}-{}", phase_word(p)),
        }
    }

    pub(crate) fn season_word(&self) -> &'static str {
        match self.season {
            SeasonName::Spring => "spring",
            SeasonName::Summer => "summer",
            SeasonName::Autumn => "autumn",
            SeasonName::Winter => "winter",
        }
    }

    /// The month of the phase (1..12) at `latitude` (degrees north): March, April, May for
    /// the northern spring, half a year later south of the equator.
    pub(crate) fn month(&self, latitude: f64) -> u32 {
        let first = match self.season {
            SeasonName::Spring => 3,
            SeasonName::Summer => 6,
            SeasonName::Autumn => 9,
            SeasonName::Winter => 12,
        };
        let step = match self.phase {
            Phase::Early => 0,
            Phase::Mid => 1,
            Phase::Late => 2,
        };
        let south = if latitude < 0.0 { 6 } else { 0 };
        (first + step + south - 1) % 12 + 1
    }

    /// The phase's typical day (month, day): early in its month for the early phase (the
    /// season has just begun), the middle for the middle, later for the late one.
    pub(crate) fn day(&self, latitude: f64) -> (u32, u32) {
        let d = match self.phase {
            Phase::Early => 10,
            Phase::Mid => 15,
            Phase::Late => 20,
        };
        (self.month(latitude), d)
    }

    /// The two texture looks of the phase and the share of the plants in the second (see
    /// the module's notes): a tree turned early in the autumn is still turned later on.
    pub(crate) fn looks(&self) -> (i32, i32, f32) {
        use Phase::*;
        use SeasonName::*;
        match (self.season, self.phase) {
            // bare trees, a few in fresh leaf; then most green, a few still bare
            (Spring, Early) => (WINTER, SPRING, 0.25),
            (Spring, Mid) => (WINTER, SPRING, 0.75),
            // the fresh green, some already in the full summer leaf
            (Spring, Late) => (SPRING, SUMMER, 0.4),
            (Summer, Early) => (SPRING, SUMMER, 0.85),
            (Summer, Mid) => (SUMMER, SUMMER, 0.0),
            // the dry late summer (`SummerDry`; green where the map has no such pictures)
            (Summer, Late) => (SUMMER, SUMMER_DRY, 0.6),
            // most still green, some turned; then mostly coloured, a few still green
            (Autumn, Early) => (SUMMER, FALL, 0.25),
            (Autumn, Mid) => (SUMMER, FALL, 0.8),
            // many bare, some still in their brown leaves; in early winter the last ones
            (Autumn, Late) => (FALL, WINTER, 0.6),
            (Winter, Early) => (FALL, WINTER, 0.9),
            (Winter, _) => (WINTER, WINTER, 0.0),
        }
    }

    /// The texture season the whole map shows: the look most plants have.
    pub(crate) fn kind(&self) -> i32 {
        let (from, to, share) = self.looks();
        if share >= 0.5 { to } else { from }
    }

    /// The plants' mix of the two looks (none when the phase has one look only).
    pub(crate) fn mix(&self) -> Option<omsi_texture::SeasonMix> {
        let (from, to, share) = self.looks();
        let folder = |k: i32| omsi_map::global::Season::folder(k).map(String::from);
        (from != to && share > 0.0).then(|| omsi_texture::SeasonMix { from: folder(from), to: folder(to), share })
    }
}

pub(crate) fn phase_word(p: Phase) -> &'static str {
    match p {
        Phase::Early => "early",
        Phase::Mid => "mid",
        Phase::Late => "late",
    }
}

/// The map's latitude (degrees north) from its `timezone.txt`, else Berlin's as the sun's.
pub(crate) fn map_latitude(root: &Path, map: &str) -> f64 {
    let cfg = omsi_cfg::resolve_path(root, map);
    cfg.parent()
        .and_then(|dir| omsi_map::TimeZone::load(&omsi_cfg::resolve_path(dir, "timezone.txt")).ok())
        .and_then(|tz| tz.lat_lon())
        .map(|(lat, _)| lat)
        .unwrap_or(omsi_sim::daylight::SunPlace::default().latitude)
}

/// The date of `choice` in `year` at `latitude`, or `date` itself (`(year, month, day)`)
/// when that already lies in the phase's month: a date the player set within the phase
/// (Christmas in early winter) stays; one outside it goes to the phase's typical day.
pub(crate) fn phase_date(choice: SeasonChoice, date: (i32, u32, u32), latitude: f64) -> (i32, u32, u32) {
    let (year, month, _) = date;
    if month == choice.month(latitude) {
        return date;
    }
    let (m, d) = choice.day(latitude);
    (year, m, d)
}

/// A season chosen with `--season` (and its phase) sets the session's date: the year and
/// the time of day stay the player's (see [`phase_date`]); a saved situation keeps its own
/// date. The season is written back as [`SeasonChoice::word`] (what LAN players are told).
pub(crate) fn apply_season_date(args: &mut Args) {
    let Some(choice) = args.season.as_deref().and_then(SeasonChoice::parse) else { return };
    args.season = Some(choice.word());
    if args.situation.is_some() {
        return;
    }
    let lat = map_latitude(&args.root, &args.map);
    let c = start_clock(args);
    let code = c.date_code();
    let now = (code / 10000, (code / 100 % 100) as u32, (code % 100) as u32);
    let (y, m, d) = phase_date(choice, now, lat);
    if (y, m, d) != now {
        args.date = Some(format!("{y:04}-{m:02}-{d:02}"));
        args.day_of_year = None;
    }
    log::info!(
        "season {}: date {y:04}-{m:02}-{d:02} ({} hemisphere, latitude {lat:.1}), texture season kind {}, plants mixed {:?}",
        choice.word(),
        if lat < 0.0 { "southern" } else { "northern" },
        choice.kind(),
        choice.mix().map(|m| (m.from, m.to, m.share))
    );
}

/// The launcher's season and phase (`autumn`, `late`) as the game reads them.
pub(crate) fn launcher_choice(season: &str, phase: &str) -> Option<SeasonChoice> {
    SeasonChoice::parse(&format!("{season}-{phase}"))
}

/// The launcher's date (`YYYY-MM-DD`) for its season and phase on `map`: the phase's
/// typical day of its year, or with `keep_in_phase` the date itself when it lies in the
/// phase already (see [`phase_date`]). None with no season chosen.
pub(crate) fn launcher_date(season: &str, phase: &str, date: &str, map: &str, keep_in_phase: bool) -> Option<String> {
    let choice = launcher_choice(season, phase)?;
    let num = |r: std::ops::Range<usize>| date.get(r).and_then(|x| x.parse::<u32>().ok());
    let lat = map_latitude(Path::new(&omsi_launcher_lib::load_config().root), map);
    let year = num(0..4).unwrap_or(1989) as i32;
    let (y, m, d) = match (num(5..7), num(8..10)) {
        (Some(m), Some(d)) if keep_in_phase => phase_date(choice, (year, m, d), lat),
        _ => {
            let (m, d) = choice.day(lat);
            (year, m, d)
        }
    };
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(s: &str) -> SeasonChoice {
        SeasonChoice::parse(s).unwrap()
    }

    #[test]
    fn parses_seasons_and_phases() {
        assert_eq!(ch("autumn"), SeasonChoice { season: SeasonName::Autumn, phase: Phase::Mid });
        assert_eq!(ch("Autumn-Late"), SeasonChoice { season: SeasonName::Autumn, phase: Phase::Late });
        assert_eq!(ch("fall_early").phase, Phase::Early);
        assert_eq!(ch("herbst spät").season, SeasonName::Autumn);
        assert_eq!(ch("fruehling-frueh"), SeasonChoice { season: SeasonName::Spring, phase: Phase::Early });
        assert!(SeasonChoice::parse("auto").is_none());
        assert!(SeasonChoice::parse("winter-soon").is_none());
        assert!(SeasonChoice::parse("").is_none());
        // what goes over the LAN: the season alone for the middle (older versions read it)
        assert_eq!(ch("autumn-mid").word(), "autumn");
        assert_eq!(ch("spring-early").word(), "spring-early");
        assert!(ch("spring-early").word().len() <= 16);
    }

    #[test]
    fn phases_are_the_months_of_the_meteorological_seasons() {
        let north = 52.5;
        let months: Vec<u32> = ["spring", "summer", "autumn", "winter"]
            .iter()
            .flat_map(|s| ["early", "mid", "late"].map(|p| ch(&format!("{s}-{p}")).month(north)))
            .collect();
        assert_eq!(months, [3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 1, 2]);
        assert_eq!(ch("spring-early").day(north), (3, 10));
        assert_eq!(ch("summer").day(north), (7, 15));
        assert_eq!(ch("winter-late").day(north), (2, 20));
        // south of the equator half a year later
        let south = -33.9;
        assert_eq!(ch("spring-early").day(south), (9, 10));
        assert_eq!(ch("winter-early").day(south), (6, 10));
        assert_eq!(ch("autumn-late").day(south), (5, 20));
        assert_eq!(ch("summer-early").day(south), (12, 10));
    }

    #[test]
    fn a_date_in_the_phase_stays_and_the_year_is_kept() {
        let w = ch("winter-early");
        assert_eq!(phase_date(w, (1989, 12, 24), 52.5), (1989, 12, 24));
        assert_eq!(phase_date(w, (1989, 5, 30), 52.5), (1989, 12, 10));
        assert_eq!(phase_date(ch("winter-mid"), (1989, 5, 30), 52.5), (1989, 1, 15));
        assert_eq!(phase_date(ch("autumn-late"), (2024, 3, 1), -34.0), (2024, 5, 20));
    }

    #[test]
    fn leap_years_count_their_29th_of_february() {
        let day_of_year = |y: i32, (m, d): (u32, u32)| {
            let mut c = omsi_sim::SimClock::default();
            c.set_date(y, m as i32, d as i32);
            c.day_of_year
        };
        let spring = ch("spring-early").day(52.5);
        assert_eq!(day_of_year(2023, spring), 69);
        assert_eq!(day_of_year(2024, spring), 70);
        // late winter's day is in February either way
        assert_eq!(day_of_year(2024, ch("winter-late").day(52.5)), 51);
        assert_eq!(phase_date(ch("winter-late"), (2024, 2, 29), 52.5), (2024, 2, 29));
    }

    #[test]
    fn texture_seasons_of_the_phases() {
        let kind = |s: &str| ch(s).kind();
        assert_eq!(kind("spring-early"), WINTER);
        assert_eq!(kind("spring-mid"), SPRING);
        assert_eq!(kind("spring-late"), SPRING);
        assert_eq!(kind("summer-early"), SUMMER);
        assert_eq!(kind("summer"), SUMMER);
        assert_eq!(kind("summer-late"), SUMMER_DRY);
        assert_eq!(kind("autumn-early"), SUMMER);
        assert_eq!(kind("autumn"), FALL);
        assert_eq!(kind("autumn-late"), WINTER);
        assert_eq!(kind("winter-early"), WINTER);
        assert_eq!(kind("winter"), WINTER);
        assert_eq!(kind("winter-late"), WINTER);
        // the mixes: early autumn a quarter of the plants in Fall, the rest summer's
        let m = ch("autumn-early").mix().unwrap();
        assert_eq!((m.from, m.to.as_deref(), m.share), (None, Some("fall"), 0.25));
        assert!(ch("summer").mix().is_none());
        assert!(ch("winter").mix().is_none());
        let late = ch("autumn-late").mix().unwrap();
        assert_eq!((late.from.as_deref(), late.to.as_deref()), (Some("fall"), Some("Winter")));
    }
}
