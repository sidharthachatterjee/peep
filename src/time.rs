//! Times as shown beside messages, in the Mac's time zone.

const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];
const WEEKDAYS: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

/// A moment on the wall clock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Local {
    pub year: i32,
    /// 1 to 12.
    pub month: u32,
    pub day: u32,
    /// 0 is Sunday.
    pub weekday: u32,
    pub hour: u32,
    pub minute: u32,
}

impl Local {
    /// The wall-clock time at `micros` microseconds since the Unix epoch.
    pub fn at(micros: i64) -> Self {
        let seconds: libc::time_t = micros.div_euclid(1_000_000);
        // SAFETY: `tm` is plain data that `localtime_r` fills in; both pointers are valid
        // for the call and nothing keeps them afterwards.
        let tm = unsafe {
            let mut tm: libc::tm = std::mem::zeroed();
            libc::localtime_r(&seconds, &mut tm);
            tm
        };
        let part = |value: libc::c_int| u32::try_from(value).unwrap_or(0);
        Self {
            year: tm.tm_year + 1900,
            month: part(tm.tm_mon) + 1,
            day: part(tm.tm_mday),
            weekday: part(tm.tm_wday),
            hour: part(tm.tm_hour),
            minute: part(tm.tm_min),
        }
    }

    pub fn now() -> Self {
        Self::at((crate::session::now() * 1_000_000.) as i64)
    }

    /// The time of day, as in `09:05`.
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}", self.hour, self.minute)
    }

    /// Whether `self` and `other` fall on the same day.
    pub fn same_day(&self, other: &Self) -> bool {
        (self.year, self.month, self.day) == (other.year, other.month, other.day)
    }

    /// The day as a heading, seen from `today`: `Today`, `Yesterday`, `Mon 3 Nov`, or
    /// `3 Nov 2025` for another year.
    pub fn day_label(&self, today: &Self) -> String {
        let month = MONTHS[(self.month as usize).clamp(1, 12) - 1];
        match today.day_number() - self.day_number() {
            0 => "Today".into(),
            1 => "Yesterday".into(),
            _ if self.year == today.year => {
                let weekday = WEEKDAYS[self.weekday as usize % 7];
                format!("{weekday} {} {month}", self.day)
            }
            _ => format!("{} {month} {}", self.day, self.year),
        }
    }

    /// Days since the Unix epoch, for telling how far apart two days are.
    fn day_number(&self) -> i64 {
        let (year, month, day) = (
            i64::from(self.year),
            i64::from(self.month),
            i64::from(self.day),
        );
        let year = if month <= 2 { year - 1 } else { year };
        let era = year.div_euclid(400);
        let year_of_era = year.rem_euclid(400);
        let day_of_year = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
        let day_of_era = year_of_era * 365 + year_of_era / 4 - year_of_era / 100 + day_of_year;
        era * 146_097 + day_of_era - 719_468
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(year: i32, month: u32, day: u32, weekday: u32) -> Local {
        Local {
            year,
            month,
            day,
            weekday,
            hour: 9,
            minute: 5,
        }
    }

    #[test]
    fn a_day_is_named_by_how_far_back_it_is() {
        let today = day(2026, 3, 1, 0);

        assert_eq!(today.day_label(&today), "Today");
        assert_eq!(day(2026, 2, 28, 6).day_label(&today), "Yesterday");
        assert_eq!(day(2026, 2, 27, 5).day_label(&today), "Fri 27 Feb");
        assert_eq!(day(2025, 12, 31, 3).day_label(&today), "31 Dec 2025");
        assert_eq!(
            day(2025, 12, 31, 3).day_label(&day(2026, 1, 1, 4)),
            "Yesterday",
            "across a new year"
        );
    }

    #[test]
    fn the_clock_pads_to_two_digits() {
        assert_eq!(day(2026, 3, 1, 0).clock(), "09:05");
    }

    #[test]
    fn day_numbers_count_from_the_epoch() {
        assert_eq!(day(1970, 1, 1, 4).day_number(), 0);
        assert_eq!(day(2024, 2, 29, 4).day_number(), 19_782);
        assert_eq!(day(2024, 3, 1, 5).day_number(), 19_783);
    }

    #[test]
    fn a_moment_is_read_in_local_time() {
        let noon = Local::at(1_700_049_600_000_000);
        assert_eq!(noon.year, 2023);
        assert_eq!(noon.month, 11);
        assert!(noon.same_day(&Local::at(1_700_049_600_000_000 + 60_000_000)));
    }
}
