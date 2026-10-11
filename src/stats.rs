//! Typing stats: how many keys today. A count only, never which keys.

use std::ffi::c_char;

#[derive(Default)]
pub struct Stats {
    pub day: i64, // local day number that `n` belongs to
    pub n: i64,   // keys typed on `day`
}

impl Stats {
    /// Starts a new count if the date changed.
    pub fn roll(&mut self, today: i64) {
        if self.day != today {
            *self = Stats { day: today, n: 0 };
        }
    }
}

#[repr(C)]
struct Tm {
    fields: [i32; 9],
    gmtoff: i64,
    zone: *const c_char,
}

unsafe extern "C" {
    fn time(t: *mut i64) -> i64;
    fn localtime_r(t: *const i64, out: *mut Tm) -> *mut Tm;
}

/// Days since 1970 in the local time zone.
pub fn today() -> i64 {
    unsafe {
        let t = time(std::ptr::null_mut());
        let mut tm: Tm = std::mem::zeroed();
        localtime_r(&t, &mut tm);
        (t + tm.gmtoff).div_euclid(86400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn count_survives_the_same_day_and_resets_on_a_new_one() {
        let mut s = Stats::default();
        s.roll(100);
        s.n = 60;
        s.roll(100);
        assert_eq!(s.n, 60);
        s.roll(101);
        assert_eq!((s.day, s.n), (101, 0));
    }

    #[test]
    fn today_is_a_plausible_local_day() {
        assert!((20_000..40_000).contains(&today()));
    }
}
