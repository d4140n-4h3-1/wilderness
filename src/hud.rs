//! The wind gauge, in the bottom left corner, out of the way of the help line at the top: an arrow showing which way the wind blows past
//! the player, turned with the view (straight up is the way the player is looking), and where it
//! comes from on the compass, and how fast, as a weather report would give it.
//!
//! North is +z and east -x: the world is right-handed, so that facing north, east is on the
//! right. The speed is the wind's push where the player stands, gusts and all,
//! taken at [`METERS_PER_SECOND`] for a push of 1: the plants' bake was made for a push of about a
//! half, a gentle breeze.

use crate::wind::Wind;
use fyrox::{
    core::{
        algebra::{Matrix3, Vector2, Vector3},
        color::Color,
        pool::Handle,
    },
    gui::{
        HorizontalAlignment, Orientation, Thickness, UserInterface, VerticalAlignment,
        border::BorderBuilder,
        brush::Brush,
        grid::{Column, Grid, GridBuilder, Row},
        stack_panel::StackPanelBuilder,
        text::{Text, TextBuilder, TextMessage},
        vector_image::{Primitive, VectorImage, VectorImageBuilder},
        widget::{WidgetBuilder, WidgetMessage},
    },
};

/// How fast the wind blows, in meters a second, for a push of 1.
pub const METERS_PER_SECOND: f32 = 8.0;
/// How big the gauge's dial is, in pixels.
const DIAL: f32 = 64.0;
/// How long the shown wind takes to follow the real one, in seconds: steady enough to read, quick
/// enough to show a gust come and go.
const SETTLE: f32 = 0.35;
/// How often the words are written again, in seconds.
const REWRITE: f32 = 0.25;

#[derive(Debug, Default, Clone, PartialEq)]
pub struct WindGauge {
    /// A grid over the whole screen, holding the gauge in its bottom row. The user interface's
    /// root lays its children out at their own size, wherever they align, so the grid is kept the
    /// screen's size by hand.
    screen: Handle<Grid>,
    screen_size: Vector2<f32>,
    arrow: Handle<VectorImage>,
    words: Handle<Text>,
    /// The wind as shown, along x and z, following the real one.
    shown: Vector2<f32>,
    since_written: f32,
}

impl WindGauge {
    pub fn build(ui: &mut UserInterface) -> Self {
        let ctx = &mut ui.build_ctx();
        let middle = Vector2::repeat(DIAL / 2.0);
        let ring = VectorImageBuilder::new(
            WidgetBuilder::new()
                .with_width(DIAL)
                .with_height(DIAL)
                .with_foreground(Brush::Solid(Color::opaque(200, 210, 220)).into()),
        )
        .with_primitives(vec![
            Primitive::WireCircle {
                center: middle,
                radius: DIAL / 2.0 - 2.0,
                thickness: 2.0,
                segments: 48,
            },
            // A tick where the view looks.
            Primitive::Line {
                begin: Vector2::new(middle.x, 1.0),
                end: Vector2::new(middle.x, 7.0),
                thickness: 2.0,
            },
        ])
        .build(ctx);
        let arrow = VectorImageBuilder::new(
            WidgetBuilder::new()
                .with_width(DIAL)
                .with_height(DIAL)
                .with_foreground(Brush::Solid(Color::WHITE).into()),
        )
        .with_primitives(vec![
            Primitive::Line {
                begin: Vector2::new(middle.x, DIAL - 14.0),
                end: Vector2::new(middle.x, 22.0),
                thickness: 4.0,
            },
            Primitive::Triangle {
                points: [
                    Vector2::new(middle.x, 10.0),
                    Vector2::new(middle.x - 10.0, 26.0),
                    Vector2::new(middle.x + 10.0, 26.0),
                ],
            },
        ])
        .build(ctx);
        let dial = GridBuilder::new(WidgetBuilder::new().with_child(ring).with_child(arrow))
            .add_row(Row::stretch())
            .add_column(Column::stretch())
            .build(ctx);
        let words = TextBuilder::new(
            WidgetBuilder::new()
                .with_margin(Thickness::left(10.0))
                .with_vertical_alignment(VerticalAlignment::Center)
                .with_foreground(Brush::Solid(Color::WHITE).into()),
        )
        .with_font_size(17.0.into())
        .with_text("Wind")
        .build(ctx);
        let reading = StackPanelBuilder::new(
            WidgetBuilder::new()
                .with_margin(Thickness {
                    left: 8.0,
                    top: 6.0,
                    right: 14.0,
                    bottom: 6.0,
                })
                .with_child(dial)
                .with_child(words),
        )
        .with_orientation(Orientation::Horizontal)
        .build(ctx);
        // On a dark, see-through backing, to read over sunlit ground.
        let gauge = BorderBuilder::new(
            WidgetBuilder::new()
                .on_row(1)
                .with_margin(Thickness::uniform(14.0))
                .with_horizontal_alignment(HorizontalAlignment::Left)
                .with_background(Brush::Solid(Color::from_rgba(10, 14, 18, 120)).into())
                .with_child(reading),
        )
        .with_corner_radius(8.0.into())
        .with_stroke_thickness(Thickness::uniform(0.0).into())
        .build(ctx);
        // Held to the bottom of the screen by a grid over all of it.
        let screen = GridBuilder::new(WidgetBuilder::new().with_child(gauge))
            .add_row(Row::stretch())
            .add_row(Row::auto())
            .add_column(Column::stretch())
            .build(ctx);
        Self {
            screen,
            arrow,
            words,
            ..Default::default()
        }
    }

    /// Shows `wind` where the player stands, at `at`, looking along `look` with `side` on their
    /// right. `blowing` is whether the plants are swaying.
    pub fn update(
        &mut self,
        ui: &mut UserInterface,
        wind: &Wind,
        blowing: bool,
        at: Vector3<f32>,
        look: Vector3<f32>,
        side: Vector3<f32>,
        dt: f32,
    ) {
        let screen_size = ui.screen_size();
        if screen_size != self.screen_size {
            self.screen_size = screen_size;
            ui.send(self.screen, WidgetMessage::Width(screen_size.x));
            ui.send(self.screen, WidgetMessage::Height(screen_size.y));
        }

        let now = wind.at(at.x, at.z);
        self.shown += (now - self.shown) * (1.0 - (-dt / SETTLE).exp());

        // Turned so that straight up is ahead: clockwise, as the screen's y runs down, by the
        // angle from ahead round to the wind.
        let (ahead, right) = (look.xz().normalize(), side.xz().normalize());
        let turn = self.shown.dot(&right).atan2(self.shown.dot(&ahead));
        let (sin, cos) = turn.sin_cos();
        let c = DIAL / 2.0;
        let transform = Matrix3::new(cos, -sin, c - c * cos + c * sin, sin, cos, c - c * sin - c * cos, 0.0, 0.0, 1.0);
        ui.send(self.arrow, WidgetMessage::RenderTransform(transform));

        self.since_written += dt;
        if self.since_written < REWRITE {
            return;
        }
        self.since_written = 0.0;
        let words = if blowing {
            describe(self.shown)
        } else {
            "Wind stilled (WILDERNESS_WIND)".to_owned()
        };
        ui.send(self.words, TextMessage::Text(words));
    }
}

/// The wind blowing along `towards` (x and z, as a push), as a weather report would give it.
pub fn describe(towards: Vector2<f32>) -> String {
    let speed = towards.norm() * METERS_PER_SECOND;
    if speed < 0.3 {
        return "Wind calm".to_owned();
    }
    let from = -towards;
    // Clockwise from north (+z) through east (-x).
    // (Plus nought, which turns a negative zero, written "-0", into zero.)
    let bearing = (-from.x).atan2(from.y).to_degrees().round().rem_euclid(360.0) + 0.0;
    format!(
        "Wind from {} ({:03.0}°)  ·  {:.1} m/s ({:.0} km/h)  ·  {}",
        compass_point(bearing),
        bearing,
        speed,
        speed * 3.6,
        beaufort(speed),
    )
}

fn compass_point(bearing: f32) -> &'static str {
    const POINTS: [&str; 16] = [
        "N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW",
    ];
    POINTS[((bearing / 22.5).round() as usize) % 16]
}

/// The Beaufort scale's name for a wind of `speed` meters a second.
fn beaufort(speed: f32) -> &'static str {
    const SCALE: [(f32, &str); 8] = [
        (0.5, "calm"),
        (1.6, "light air"),
        (3.4, "light breeze"),
        (5.5, "gentle breeze"),
        (8.0, "moderate breeze"),
        (10.8, "fresh breeze"),
        (13.9, "strong breeze"),
        (17.2, "near gale"),
    ];
    SCALE.iter().find(|(top, _)| speed < *top).map_or("gale", |(_, name)| name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn describes_where_the_wind_comes_from() {
        // Blowing towards the north-east comes from the south-west.
        let words = describe(Vector2::new(-0.5, 0.5));
        assert!(words.starts_with("Wind from SW (225°)"), "{words}");
        assert!(words.contains("5.7 m/s") && words.contains("moderate breeze"), "{words}");
        assert!(describe(Vector2::new(0.0, -0.3)).starts_with("Wind from N (000°)"));
        assert_eq!(describe(Vector2::zeros()), "Wind calm");
    }
}
