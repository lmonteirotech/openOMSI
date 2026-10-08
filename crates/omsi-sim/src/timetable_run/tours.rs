//! A tour's key, its vehicle's file name as `car_use` writes it, and the days it runs on.

/// A 64-bit hash of a tour: its depot group (lower case), line and tour number.
pub fn tour_key_of(group: &str, line: &str, tour: &str) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in group.bytes().chain([0]).chain(line.trim().bytes()).chain([0]).chain(tour.trim().bytes()) {
        h = (h ^ b as u64).wrapping_mul(0x0000_0100_0000_01b3);
    }
    mix(h)
}

pub fn mix(mut h: u64) -> u64 {
    h ^= h >> 33;
    h = h.wrapping_mul(0xff51_afd7_ed55_8ccd);
    h ^ (h >> 33)
}

/// A vehicle file path compared as `car_use` writes it (`vehicles\MAN_SD200\MAN_SD77.bus`):
/// lower case with forward slashes.
pub fn norm_vehicle_path(p: &str) -> String {
    p.trim().replace('\\', "/").to_ascii_lowercase()
}

/// The tour mask bits `clock`'s date selects: (the weekday's or public holiday's, the school
/// holidays' or school days'). the original: bit 8 = runs in the school holidays,
/// bit 9 = runs on school days.
pub fn day_bits(calendar: &omsi_map::Calendar, clock: &crate::SimClock) -> (i32, i32) {
    let date = clock.date_code();
    let day_bit = if calendar.is_holiday(date) { 1 << 7 } else { 1 << clock.weekday() };
    let school_bit = if calendar.in_holiday_range(date) { 1 << 8 } else { 1 << 9 };
    (day_bit, school_bit)
}
