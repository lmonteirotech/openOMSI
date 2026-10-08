//! Every script variable of a player's vehicle, for the copies the others draw (`VARS`).
//!
//! The pose (`wire.rs`) carries what every copy needs at the full rate: where the bus is, its
//! lamps, switches and the values its sounds and moving parts follow. Everything else the
//! scripts work out - a gearbox's state, a display's mode, an IBIS, a ticket printer, a
//! variable a plugin or a `setvar` set - went nowhere, and another player's bus showed what
//! its own copy of the scripts made of it. This sends all of them: ten times a second the
//! variables that changed since they were last sent, and all of them in turn in between (a
//! "key frame" a slice at a time), so a lost datagram is mended within seconds and a player
//! who joins late sees the whole state. Strings go the same way.
//!
//! A message names the vehicle's variable table by a hash of its variables' names (the
//! game's): a copy made from other files (another version of the bus) takes nothing.
//!
//! ```text
//! byte 0     0xB5 (no text message starts with it)
//! byte 1     protocol
//! bytes 2-5  sender's id (u32 LE)
//! bytes 6-9  table hash (u32 LE)
//! byte 10    kind: 0 floats by index, 1 a run of floats, 2 strings by index
//! then       0: count u16, (index u16, f32)*      1: first u16, count u16, f32*
//!            2: count u16, (index u16, length u16, UTF-8)*
//! ```
//! All numbers little-endian. Indices are the receiver's `VarId` / `StrVarId` for the same
//! table.

/// The first byte of a `VARS` datagram.
pub const VARS_MAGIC: u8 = 0xB5;
const HEADER: usize = 11;
/// What one datagram holds at most (within `MAX_DATAGRAM`, with room to spare).
const ROOM: usize = 1300;
/// How often changes go out (s), and how many ticks between key frame slices.
const EVERY: f32 = 0.1;
const KEY_EVERY: u32 = 5;
const STRINGS_KEY_EVERY: u32 = 10;
/// A string longer than this goes cut (a display's text, a path).
const MAX_STRING: usize = 255;

/// Variables of a player's vehicle as they came (`LanSession::take_vars`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct VarsIn {
    pub id: u32,
    pub table: u32,
    pub floats: Vec<(u16, f32)>,
    pub strings: Vec<(u16, String)>,
}

/// Our side: what was sent last, and where the key frame has got to.
#[derive(Debug, Default)]
pub struct VarSender {
    table: u32,
    floats: Vec<f32>,
    strings: Vec<String>,
    acc: f32,
    ticks: u32,
    delta_from: usize,
    key_float: usize,
    key_string: usize,
}

fn header(protocol: u8, id: u32, table: u32, kind: u8) -> Vec<u8> {
    let mut d = Vec::with_capacity(ROOM + HEADER);
    d.push(VARS_MAGIC);
    d.push(protocol);
    d.extend_from_slice(&id.to_le_bytes());
    d.extend_from_slice(&table.to_le_bytes());
    d.push(kind);
    d
}

fn same(a: f32, b: f32) -> bool {
    a.to_bits() == b.to_bits() || (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1.0e-6 * a.abs().max(1.0)
}

/// A string as it goes (cut at a character boundary).
fn cut(s: &str) -> &str {
    if s.len() <= MAX_STRING {
        return s;
    }
    let mut n = MAX_STRING;
    while !s.is_char_boundary(n) {
        n -= 1;
    }
    &s[..n]
}

impl VarSender {
    /// Our vehicle's variables (`ids[k]` holds `values[k]`, the same for strings) for this
    /// frame: the datagrams due now (none most frames).
    #[allow(clippy::too_many_arguments)]
    pub fn tick(&mut self, protocol: u8, id: u32, table: u32, float_ids: &[u16], floats: &[f32], string_ids: &[u16], strings: &[String], dt: f32) -> Vec<Vec<u8>> {
        if table != self.table || self.floats.len() != floats.len() || self.strings.len() != strings.len() {
            // another vehicle (or another table): everything is new
            *self = VarSender { table, floats: vec![f32::NAN; floats.len()], strings: vec!["\u{0}".into(); strings.len()], acc: EVERY, ..VarSender::default() };
        }
        self.acc += dt;
        if self.acc < EVERY {
            return Vec::new();
        }
        self.acc = 0.0;
        self.ticks = self.ticks.wrapping_add(1);
        let mut out = Vec::new();
        let n = floats.len().min(float_ids.len());
        // what changed, from where the last datagram had to stop
        if n > 0 {
            let mut d = header(protocol, id, table, 0);
            d.extend_from_slice(&0u16.to_le_bytes());
            let mut count = 0u16;
            let mut k = self.delta_from % n;
            let mut looked = 0;
            while looked < n && d.len() + 6 <= ROOM {
                if !same(floats[k], self.floats[k]) {
                    d.extend_from_slice(&float_ids[k].to_le_bytes());
                    d.extend_from_slice(&floats[k].to_le_bytes());
                    self.floats[k] = floats[k];
                    count += 1;
                }
                k = (k + 1) % n;
                looked += 1;
            }
            self.delta_from = k;
            if count > 0 {
                d[HEADER..HEADER + 2].copy_from_slice(&count.to_le_bytes());
                out.push(d);
            }
        }
        // a slice of the key frame: a run of consecutive variables as they are now
        if n > 0 && self.ticks % KEY_EVERY == 0 {
            let first = self.key_float % n;
            let take = ((ROOM - HEADER - 4) / 4).min(n - first);
            // (only a run of ids one after another can go as a run: they mostly are)
            let mut run = 1;
            while run < take && float_ids[first + run] == float_ids[first] + run as u16 {
                run += 1;
            }
            let mut d = header(protocol, id, table, 1);
            d.extend_from_slice(&float_ids[first].to_le_bytes());
            d.extend_from_slice(&(run as u16).to_le_bytes());
            for k in first..first + run {
                d.extend_from_slice(&floats[k].to_le_bytes());
                self.floats[k] = floats[k];
            }
            self.key_float = (first + run) % n;
            out.push(d);
        }
        // strings: those that changed, and in turn the others
        let m = strings.len().min(string_ids.len());
        if m > 0 {
            let key = self.ticks % STRINGS_KEY_EVERY == 0;
            let mut d = header(protocol, id, table, 2);
            d.extend_from_slice(&0u16.to_le_bytes());
            let mut count = 0u16;
            let put = |d: &mut Vec<u8>, k: usize, count: &mut u16| -> bool {
                let s = cut(&strings[k]);
                if d.len() + 4 + s.len() > ROOM {
                    return false;
                }
                d.extend_from_slice(&string_ids[k].to_le_bytes());
                d.extend_from_slice(&(s.len() as u16).to_le_bytes());
                d.extend_from_slice(s.as_bytes());
                *count += 1;
                true
            };
            for k in 0..m {
                if strings[k] != self.strings[k] && put(&mut d, k, &mut count) {
                    self.strings[k] = strings[k].clone();
                }
            }
            if key {
                let mut k = self.key_string % m;
                for _ in 0..m {
                    if !put(&mut d, k, &mut count) {
                        break;
                    }
                    self.strings[k] = strings[k].clone();
                    k = (k + 1) % m;
                }
                self.key_string = k;
            }
            if count > 0 {
                d[HEADER..HEADER + 2].copy_from_slice(&count.to_le_bytes());
                out.push(d);
            }
        }
        out
    }
}

/// A `VARS` datagram read (None: not one, another protocol, or cut short).
pub fn decode(data: &[u8], protocol: u8) -> Option<VarsIn> {
    if data.len() < HEADER + 2 || data[0] != VARS_MAGIC || data[1] != protocol {
        return None;
    }
    let u16_at = |at: usize| -> Option<u16> { Some(u16::from_le_bytes(data.get(at..at + 2)?.try_into().ok()?)) };
    let f32_at = |at: usize| -> Option<f32> { Some(f32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?)) };
    let id = u32::from_le_bytes(data[2..6].try_into().ok()?);
    let table = u32::from_le_bytes(data[6..10].try_into().ok()?);
    let mut v = VarsIn { id, table, ..VarsIn::default() };
    let mut at = HEADER;
    match data[10] {
        0 => {
            let count = u16_at(at)? as usize;
            at += 2;
            for _ in 0..count {
                v.floats.push((u16_at(at)?, f32_at(at + 2)?));
                at += 6;
            }
        }
        1 => {
            let first = u16_at(at)?;
            let count = u16_at(at + 2)?;
            at += 4;
            for k in 0..count {
                v.floats.push((first.checked_add(k)?, f32_at(at)?));
                at += 4;
            }
        }
        2 => {
            let count = u16_at(at)? as usize;
            at += 2;
            for _ in 0..count {
                let idx = u16_at(at)?;
                let len = u16_at(at + 2)? as usize;
                let s = std::str::from_utf8(data.get(at + 4..at + 4 + len)?).ok()?;
                v.strings.push((idx, s.to_string()));
                at += 4 + len;
            }
        }
        _ => return None,
    }
    Some(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What goes out is what comes in: the changes at once, the rest by key frame slices,
    /// strings too - and after a few seconds every variable has arrived, whatever was lost.
    #[test]
    fn every_variable_arrives_and_changes_go_at_once() {
        let n = 3000;
        let ids: Vec<u16> = (0..n as u16).collect();
        let mut floats: Vec<f32> = (0..n).map(|k| k as f32 * 0.5).collect();
        let sids: Vec<u16> = (0..40).collect();
        let mut strings: Vec<String> = (0..40).map(|k| format!("text {k}")).collect();
        let mut tx = VarSender::default();
        let mut got = vec![f32::NAN; n];
        let mut got_s = vec![String::new(); 40];
        let take = |msgs: Vec<Vec<u8>>, got: &mut Vec<f32>, got_s: &mut Vec<String>| {
            for m in msgs {
                assert!(m.len() <= crate::MAX_DATAGRAM, "{}", m.len());
                let v = decode(&m, 5).unwrap();
                assert_eq!((v.id, v.table), (7, 99));
                for (i, x) in v.floats {
                    got[i as usize] = x;
                }
                for (i, s) in v.strings {
                    got_s[i as usize] = s;
                }
            }
        };
        // the first message after a change carries it
        take(tx.tick(5, 7, 99, &ids, &floats, &sids, &strings, 0.1), &mut got, &mut got_s);
        floats[2500] = -1.0;
        strings[3] = "Hauptbahnhof".into();
        take(tx.tick(5, 7, 99, &ids, &floats, &sids, &strings, 0.1), &mut got, &mut got_s);
        // (the first tick sent everything changed from "unknown", as far as a datagram holds)
        for _ in 0..200 {
            take(tx.tick(5, 7, 99, &ids, &floats, &sids, &strings, 0.1), &mut got, &mut got_s);
        }
        assert_eq!(got, floats);
        assert_eq!(got_s, strings);
        // an unchanged state sends little: the key frame slices only
        let quiet: usize = (0..10).map(|_| tx.tick(5, 7, 99, &ids, &floats, &sids, &strings, 0.1).iter().map(Vec::len).sum::<usize>()).sum();
        assert!(quiet < 4 * ROOM, "{quiet} bytes a second for nothing new");
        // one change: in the very next datagram
        floats[17] = 4.25;
        let m = tx.tick(5, 7, 99, &ids, &floats, &sids, &strings, 0.1);
        let v = decode(&m[0], 5).unwrap();
        assert_eq!(v.floats, vec![(17, 4.25)]);
    }

    #[test]
    fn a_short_or_foreign_datagram_is_not_taken() {
        let mut tx = VarSender::default();
        let m = tx.tick(5, 1, 2, &[0, 1], &[1.0, 2.0], &[0], &["x".into()], 0.1);
        assert!(decode(&m[0], 4).is_none(), "another protocol");
        assert!(decode(&m[0][..m[0].len() - 1], 5).is_none(), "cut short");
        assert!(decode(b"STATE|1", 5).is_none());
    }
}
