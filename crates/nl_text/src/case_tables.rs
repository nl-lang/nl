//! Rust's Unicode case mappings as compact text, for a string library
//! written in Lua: the stand-in of `runtime/nl_80_string.lua` reads them
//! (`host.case_tables`) and then maps case as [`crate::map_case`] does,
//! where the text library itself cannot be loaded (stock LuaJIT, Garry's
//! Mod clients).
//!
//! # Encoding
//!
//! Five strings of ASCII text; numbers are hexadecimal.
//!
//! - `up`, `down`: the characters whose `to_uppercase` (`to_lowercase`) is
//!   one other character, as runs `start count step delta;`: the `count`
//!   characters `start`, `start + step`, … each map to themselves plus
//!   `delta`, which is written with its sign (`+20`, `-20`).
//! - `upx`, `downx`: the characters that map to several, as
//!   `char:first.second[.third];`.
//! - `swapx`: the characters whose `swapcase` is not what the mappings
//!   give by default (the lowercase mapping when there is one, otherwise
//!   the uppercase one, otherwise the character): `char:result[.more];`.
//!   These are the characters that are neither uppercase nor lowercase but
//!   have a mapping, such as the titlecase `ǅ`, which `swapcase` keeps.
//!
//! The one contextual rule of `str::to_lowercase`, a final capital sigma,
//! is not in the tables: the reader applies it.

/// The encoded tables (see the module documentation).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaseTables {
    pub up: String,
    pub upx: String,
    pub down: String,
    pub downx: String,
    pub swapx: String,
}

/// What [`crate::map_case`]'s swap does to one character.
fn swap_of(c: char) -> Vec<char> {
    if c.is_uppercase() {
        c.to_lowercase().collect()
    } else if c.is_lowercase() {
        c.to_uppercase().collect()
    } else {
        vec![c]
    }
}

/// Appends the runs of single-character mappings (`pairs` sorted by
/// character) to `out`.
fn runs(pairs: &[(u32, u32)], out: &mut String) {
    use std::fmt::Write as _;
    let mut i = 0;
    while let Some(&(start, to)) = pairs.get(i) {
        let delta = i64::from(to) - i64::from(start);
        // The longest run from here with one step and one delta.
        let mut best = (1usize, 1u32);
        for step in [1u32, 2] {
            let mut n = 1;
            while let Some(&(c, t)) = pairs.get(i + n) {
                if c != start + step * n as u32 || i64::from(t) - i64::from(c) != delta {
                    break;
                }
                n += 1;
            }
            if n > best.0 {
                best = (n, step);
            }
        }
        let sign = if delta < 0 { '-' } else { '+' };
        let _ = write!(out, "{start:x} {:x} {:x} {sign}{:x};", best.0, best.1, delta.unsigned_abs());
        i += best.0;
    }
}

fn multi(entries: &[(u32, Vec<char>)], out: &mut String) {
    use std::fmt::Write as _;
    for (c, to) in entries {
        let parts: Vec<String> = to.iter().map(|t| format!("{:x}", u32::from(*t))).collect();
        let _ = write!(out, "{c:x}:{};", parts.join("."));
    }
}

/// Rust's case mappings, encoded.
pub fn case_tables() -> CaseTables {
    let mut up1 = Vec::new();
    let mut upx = Vec::new();
    let mut down1 = Vec::new();
    let mut downx = Vec::new();
    let mut swapx = Vec::new();
    for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
        let u: Vec<char> = c.to_uppercase().collect();
        let l: Vec<char> = c.to_lowercase().collect();
        let cp = u32::from(c);
        match u.as_slice() {
            [x] if *x == c => {}
            [x] => up1.push((cp, u32::from(*x))),
            _ => upx.push((cp, u.clone())),
        }
        match l.as_slice() {
            [x] if *x == c => {}
            [x] => down1.push((cp, u32::from(*x))),
            _ => downx.push((cp, l.clone())),
        }
        let default = if l != [c] { l } else { u };
        let swap = swap_of(c);
        if swap != default {
            swapx.push((cp, swap));
        }
    }
    let mut t = CaseTables::default();
    runs(&up1, &mut t.up);
    multi(&upx, &mut t.upx);
    runs(&down1, &mut t.down);
    multi(&downx, &mut t.downx);
    multi(&swapx, &mut t.swapx);
    t
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn hex(s: &str) -> u32 {
        u32::from_str_radix(s, 16).unwrap()
    }

    /// Decodes the tables as the Lua reader does.
    fn decode(runs: &str, multi: &str) -> HashMap<u32, Vec<u32>> {
        let mut m = HashMap::new();
        for run in runs.split(';').filter(|r| !r.is_empty()) {
            let f: Vec<&str> = run.split(' ').collect();
            let (start, count, step) = (hex(f[0]), hex(f[1]), hex(f[2]));
            let delta = i64::from(hex(&f[3][1..])) * if f[3].starts_with('-') { -1 } else { 1 };
            for k in 0..count {
                let c = start + k * step;
                m.insert(c, vec![u32::try_from(i64::from(c) + delta).unwrap()]);
            }
        }
        for e in multi.split(';').filter(|r| !r.is_empty()) {
            let (c, to) = e.split_once(':').unwrap();
            m.insert(hex(c), to.split('.').map(hex).collect());
        }
        m
    }

    /// Decoding gives back every mapping of every character, and the
    /// swap rule with its exceptions gives `swapcase`.
    #[test]
    fn tables_round_trip() {
        let t = case_tables();
        let up = decode(&t.up, &t.upx);
        let down = decode(&t.down, &t.downx);
        let swapx = decode("", &t.swapx);
        for c in (0..=0x10FFFFu32).filter_map(char::from_u32) {
            let cp = u32::from(c);
            let want_u: Vec<u32> = c.to_uppercase().map(u32::from).collect();
            let want_l: Vec<u32> = c.to_lowercase().map(u32::from).collect();
            assert_eq!(up.get(&cp).cloned().unwrap_or(vec![cp]), want_u, "{c:?}");
            assert_eq!(down.get(&cp).cloned().unwrap_or(vec![cp]), want_l, "{c:?}");
            let swap = swapx.get(&cp).or(down.get(&cp)).or(up.get(&cp)).cloned().unwrap_or(vec![cp]);
            assert_eq!(swap, swap_of(c).into_iter().map(u32::from).collect::<Vec<_>>(), "{c:?}");
        }
        // Compact: a few kilobytes in all.
        let size = t.up.len() + t.upx.len() + t.down.len() + t.downx.len() + t.swapx.len();
        assert!(size < 16 * 1024, "{size} bytes");
    }
}
