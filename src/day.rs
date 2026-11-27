//! The days going by, sped up, on the engine's calendar ([`GameCalendar`]): the sun rising,
//! crossing the sky and setting, then the moon, with the sky, the light in the shade and the haze
//! following; and the year going round with them, the sun higher in summer and lower in winter, and
//! the leaves turning (see `seasons.rs`).
//!
//! A whole day takes [`DAY_MINUTES`] and starts at [`START_HOUR`] on [`START_DATE`]:
//! WILDERNESS_DAY=<minutes a whole day takes>, WILDERNESS_HOUR=<hour it starts at> and
//! WILDERNESS_DATE=<month>-<day> say otherwise, and WILDERNESS_DAY=0 stops the clock in the
//! afternoon, as the forest was before there were days. Measuring (WILDERNESS_BENCH), the
//! afternoon stays unless WILDERNESS_DAY is set, so that runs compare. `[` and `]` step an hour
//! back or forward, `,` and `.` a day, `-` and `=` a week; `P` pauses the clock.

use fyrox::core::{
    algebra::{UnitQuaternion, Vector3},
    calendar::{CalendarDate, GameCalendar, SECONDS_PER_DAY},
    color::Color,
    log::Log,
};

/// How long a whole day takes, in minutes: an hour of it a minute.
const DAY_MINUTES: f32 = 24.0;
/// The hour a day starts at: the morning, the day ahead.
const START_HOUR: f32 = 9.0;
/// The date the forest starts on, month and day: mid-October, the birches and larches turning
/// gold, the oaks and alders still green.
pub const START_DATE: (u8, u8) = (10, 18);

/// How far north the forest is, in degrees.
const LATITUDE: f32 = 48.0;
/// How far the earth's axis is tilted, in degrees: how far north of the equator the sun stands at
/// midsummer, and south at midwinter.
const TILT: f32 = 23.44;

/// The sun high up, and low, near the horizon.
const SUN_HIGH: Color = Color::opaque(255, 244, 222);
const SUN_LOW: Color = Color::opaque(255, 150, 80);
/// The moon: full, opposite the sun, faint and blue.
const MOON: Color = Color::opaque(170, 190, 235);
const MOON_STRENGTH: f32 = 0.12;

/// By the sun's elevation in degrees: the light in the shade, the haze and the sky.
const AMBIENT: [(f32, Color); 5] = [
    (-14.0, Color::opaque(10, 14, 26)),
    (-6.0, Color::opaque(40, 50, 80)),
    (2.0, Color::opaque(110, 100, 112)),
    (15.0, Color::opaque(128, 136, 146)),
    (90.0, Color::opaque(128, 136, 146)),
];
const HAZE: [(f32, Color); 5] = [
    (-14.0, Color::opaque(12, 16, 28)),
    (-6.0, Color::opaque(50, 60, 92)),
    (2.0, Color::opaque(205, 160, 140)),
    (15.0, Color::opaque(178, 192, 204)),
    (90.0, Color::opaque(178, 192, 204)),
];
/// The sky overhead, by the sun's elevation.
const ZENITH: [(f32, Color); 5] = [
    (-14.0, Color::opaque(2, 3, 9)),
    (-6.0, Color::opaque(14, 22, 52)),
    (2.0, Color::opaque(52, 78, 140)),
    (15.0, Color::opaque(62, 112, 196)),
    (90.0, Color::opaque(52, 104, 196)),
];
const SKY: [(f32, Color); 5] = [
    (-14.0, Color::opaque(6, 9, 20)),
    (-6.0, Color::opaque(40, 55, 95)),
    (2.0, Color::opaque(230, 150, 110)),
    (15.0, Color::opaque(130, 170, 220)),
    (90.0, Color::opaque(110, 155, 215)),
];

/// The date and the time of day, going by.
#[derive(Debug, Clone, PartialEq)]
pub struct Day {
    calendar: GameCalendar,
}

/// The date WILDERNESS_DATE asks for (`<month>-<day>`, as `10-18`), or [`START_DATE`].
pub fn start_date() -> CalendarDate {
    let asked = std::env::var("WILDERNESS_DATE").ok();
    let date = asked.as_deref().and_then(|date| {
        let (month, day) = date.trim().split_once('-')?;
        CalendarDate::new(1, month.parse().ok()?, day.parse().ok()?).ok()
    });
    if let (Some(asked), None) = (&asked, date) {
        Log::warn(format!("WILDERNESS_DATE={asked} is not a month and a day, as 10-18"));
    }
    date.unwrap_or_else(|| CalendarDate::new(1, START_DATE.0, START_DATE.1).expect("a date"))
}

/// "18 Oct".
pub fn date_text(date: CalendarDate) -> String {
    const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let (month, day) = date.month_day();
    format!("{day} {}", MONTHS[month as usize - 1])
}

/// How everything is lit at one time of day.
#[derive(Debug, Clone, PartialEq)]
pub struct Lighting {
    /// The sun's height over the horizon, in degrees; negative at night.
    pub elevation: f32,
    /// Turns a directional light, which shines down its own -y, to shine from the sun, or from
    /// the moon at night.
    pub rotation: UnitQuaternion<f32>,
    pub color: Color,
    /// Times the light's intensity in full sun.
    pub strength: f32,
    pub ambient: Color,
    pub haze: Color,
    /// The sky near the horizon toward the sun: blue by day, pink and orange as it sets.
    pub sky: Color,
    /// The sky overhead.
    pub zenith: Color,
    /// The sun's own colour, whatever lights the land.
    pub sun_color: Color,
    /// Unit vectors toward the sun and the moon (full, opposite it).
    pub toward_sun: Vector3<f32>,
    pub toward_moon: Vector3<f32>,
    /// How much the sky glows toward the sun at the horizon, sunrise and sunset: 0 to 1.
    pub twilight: f32,
    /// How bright the stars are, 0 by day to 1 in the dark of night.
    pub stars: f32,
    /// How far round the stars have turned about the celestial pole, in radians.
    pub star_turn: f32,
}

impl Day {
    /// The day WILDERNESS_DAY, WILDERNESS_HOUR and WILDERNESS_DATE ask for, if they do.
    pub fn from_env() -> Option<Self> {
        let minutes: f64 = match std::env::var("WILDERNESS_DAY") {
            Ok(minutes) => minutes.parse().ok().filter(|m: &f64| *m > 0.0)?,
            Err(_) if crate::benchmarking() => return None,
            Err(_) => DAY_MINUTES as f64,
        };
        let hour: f64 = std::env::var("WILDERNESS_HOUR")
            .ok()
            .and_then(|hour| hour.parse().ok())
            .unwrap_or(START_HOUR as f64);
        let seconds = (hour * 3600.0).rem_euclid(SECONDS_PER_DAY);
        let calendar = GameCalendar::new(start_date(), seconds, SECONDS_PER_DAY / (minutes * 60.0)).ok()?;
        Some(Self { calendar })
    }

    /// Moves the clock on `dt` seconds; whether the date changed.
    pub fn advance(&mut self, dt: f32) -> bool {
        self.calendar.tick(dt as f64).unwrap_or(false)
    }

    /// Moves the clock on, or back, `hours`, paused or not; whether the date changed.
    pub fn step(&mut self, hours: f32) -> bool {
        self.calendar.advance_game_seconds(hours as f64 * 3600.0).unwrap_or(false)
    }

    pub fn toggle_pause(&mut self) {
        self.calendar.set_paused(!self.calendar.is_paused());
    }

    pub fn date(&self) -> CalendarDate {
        self.calendar.date()
    }

    /// The day of the year, 1 on January 1, with the time of day as a fraction.
    pub fn day_of_year(&self) -> f32 {
        self.calendar.fractional_day_of_year() as f32
    }

    fn hour(&self) -> f32 {
        self.calendar.hour() as f32
    }

    /// "18 Oct 06:45", and whether the clock is stopped.
    pub fn clock(&self) -> String {
        let minutes = (self.hour() * 60.0) as u32;
        format!(
            "{} {:02}:{:02}{}",
            date_text(self.date()),
            minutes / 60,
            minutes % 60,
            if self.calendar.is_paused() { " (paused)" } else { "" }
        )
    }

    pub fn lighting(&self) -> Lighting {
        let toward_sun = toward_sun(self.hour(), self.day_of_year());
        let elevation = toward_sun.y.asin().to_degrees();
        // Below the horizon the sun fades out; once it is well down the moon, opposite, fades
        // in: between, both are dark, so the light turns round unseen.
        let (toward, color, strength) = if elevation > -6.0 {
            let high = smoothstep(0.0, 20.0, elevation);
            (toward_sun, lerp(SUN_LOW, SUN_HIGH, high), smoothstep(-3.0, 5.0, elevation))
        } else {
            (-toward_sun, MOON, MOON_STRENGTH * smoothstep(6.0, 14.0, -elevation))
        };
        Lighting {
            elevation,
            // A directional light's +y points toward what lights it.
            rotation: UnitQuaternion::rotation_between(&Vector3::y(), &toward)
                .unwrap_or_else(|| UnitQuaternion::from_axis_angle(&Vector3::x_axis(), std::f32::consts::PI)),
            color,
            strength,
            ambient: by_elevation(&AMBIENT, elevation),
            haze: by_elevation(&HAZE, elevation),
            sky: by_elevation(&SKY, elevation),
            zenith: by_elevation(&ZENITH, elevation),
            sun_color: lerp(SUN_LOW, SUN_HIGH, smoothstep(0.0, 20.0, elevation)),
            toward_sun,
            toward_moon: -toward_sun,
            // Strongest with the sun at the horizon, gone once it is high or well down.
            twilight: smoothstep(-10.0, -2.0, elevation) * (1.0 - smoothstep(4.0, 14.0, elevation)),
            stars: smoothstep(-4.0, -14.0, elevation),
            // A whole turn a day, and a little more (the stars come round four minutes early).
            star_turn: (self.hour() / 24.0 * 1.0027) * std::f32::consts::TAU,
        }
    }
}

/// How far north of the equator the sun stands on a day of the year, in degrees: furthest at the
/// June solstice, about day 172, furthest south at the December one.
fn declination(day_of_year: f32) -> f32 {
    TILT * (std::f32::consts::TAU * (day_of_year - 81.0) / 365.0).sin()
}

/// A unit vector toward the sun at an hour of a day of the year; the world's north is +z, its
/// east -x.
fn toward_sun(hour: f32, day_of_year: f32) -> Vector3<f32> {
    let latitude = LATITUDE.to_radians();
    let declination = declination(day_of_year).to_radians();
    let hour_angle = ((hour - 12.0) * 15.0).to_radians();
    let east = -declination.cos() * hour_angle.sin();
    let north = declination.sin() * latitude.cos() - declination.cos() * hour_angle.cos() * latitude.sin();
    let up = declination.sin() * latitude.sin() + declination.cos() * hour_angle.cos() * latitude.cos();
    Vector3::new(-east, up, north).normalize()
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    crate::random::smoothstep(edge0, edge1, x)
}

fn lerp(a: Color, b: Color, t: f32) -> Color {
    let mix = |a: u8, b: u8| (a as f32 + (b as f32 - a as f32) * t).round() as u8;
    Color::opaque(mix(a.r, b.r), mix(a.g, b.g), mix(a.b, b.b))
}

fn by_elevation(table: &[(f32, Color)], elevation: f32) -> Color {
    if elevation <= table[0].0 {
        return table[0].1;
    }
    for pair in table.windows(2) {
        let ((e0, c0), (e1, c1)) = (pair[0], pair[1]);
        if elevation <= e1 {
            return lerp(c0, c1, (elevation - e0) / (e1 - e0));
        }
    }
    table[table.len() - 1].1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sun_rises_in_the_east_and_is_south_at_noon() {
        let midsummer = 172.0;
        let morning = toward_sun(8.0, midsummer);
        assert!(morning.x < 0.0 && morning.y > 0.0, "{morning:?}");
        let noon = toward_sun(12.0, midsummer);
        assert!(noon.z < 0.0 && noon.x.abs() < 1e-5, "{noon:?}");
        let elevation = noon.y.asin().to_degrees();
        assert!((elevation - (90.0 - LATITUDE + TILT)).abs() < 0.05, "{elevation}");
        assert!(toward_sun(0.0, midsummer).y < 0.0);
    }

    #[test]
    fn the_sun_is_lower_in_winter_and_the_days_shorter() {
        let noon = |day: f32| toward_sun(12.0, day).y.asin().to_degrees();
        assert!((noon(355.0) - (90.0 - LATITUDE - TILT)).abs() < 0.1, "{}", noon(355.0));
        assert!((noon(80.0) - (90.0 - LATITUDE)).abs() < 1.0, "{}", noon(80.0));
        // At 7 in the morning the sun is up in June, not in December.
        assert!(toward_sun(7.0, 172.0).y > 0.0 && toward_sun(7.0, 355.0).y < 0.0);
    }

    fn at(month: u8, day: u8, hour: f64) -> Day {
        let date = CalendarDate::new(1, month, day).unwrap();
        Day { calendar: GameCalendar::new(date, hour * 3600.0, 0.0).unwrap() }
    }

    #[test]
    fn light_shines_from_the_sun_by_day_and_the_moon_by_night() {
        let day = at(6, 21, 15.0);
        let lit = day.lighting();
        assert!((lit.rotation * Vector3::y() - toward_sun(15.0, day.day_of_year())).norm() < 1e-4);
        assert_eq!(lit.strength, 1.0);
        let night = at(6, 21, 0.5);
        let dark = night.lighting();
        assert!((dark.rotation * Vector3::y() + toward_sun(0.5, night.day_of_year())).norm() < 1e-4);
        assert!(dark.strength > 0.0 && dark.strength <= MOON_STRENGTH);
    }

    #[test]
    fn stepping_crosses_midnight_into_the_next_day() {
        let mut day = at(12, 31, 23.5);
        assert!(day.step(1.0));
        assert_eq!(day.date(), CalendarDate::new(2, 1, 1).unwrap());
        assert_eq!(day.clock(), "1 Jan 00:30");
        assert!(!day.step(1.0));
    }
}
