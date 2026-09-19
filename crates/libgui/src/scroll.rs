//! Scroll behaviour: see [`ScrollConfig`] for the whole of it.

/// How a scroll area closes the distance to where the input asked it to be.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Smoothing {
    /// Apply the delta in full, the frame it arrives. The right answer for a
    /// signal that is already smooth: easing it again only makes the content
    /// trail the thing you are moving.
    #[default]
    Instant,
    /// Ease towards the destination, closing this fraction of the remaining
    /// distance per second (frame-rate independent). Higher arrives sooner;
    /// 20 is about 100 ms to settle. Zero or less behaves as `Instant`.
    Eased { rate: f32 },
}

/// Everything a scroll area does with its input — and nothing about what
/// produced it.
///
/// libgui does not know, and must not guess, what you are scrolling with. A
/// trackpad, a high-resolution wheel, a trackball, a joystick axis the host
/// samples once a frame, a jog dial, a MIDI encoder, an accessibility switch:
/// all of them arrive as [`InputEvent::Wheel`](crate::InputEvent::Wheel), and
/// the only thing the core asks of the host is what one unit of that delta
/// *means*:
///
/// - [`WheelUnit::Pixel`](crate::WheelUnit::Pixel) — a **continuous** signal,
///   already in logical px. The host is sampling something that moves
///   smoothly, so there is nothing for the UI to interpolate.
/// - [`WheelUnit::Line`](crate::WheelUnit::Line) /
///   [`Page`](crate::WheelUnit::Page) — a **stepped** signal. One event stands
///   for a whole detent or page, and turning that jump into motion is the UI's
///   job.
///
/// That is a property of the signal, not of the hardware, and the host is the
/// only thing in the stack that knows it. A host with a free-spinning wheel
/// reporting eighths of a detent should send `Pixel` deltas; a host driving
/// scroll from a D-pad should send `Line`. Nothing below that line has to
/// recognise a device.
///
/// What the UI then *does* with each of the two is the app's decision, and all
/// of it is here: step sizes, the smoothing curve for each kind of signal, and
/// fling deceleration. Set it once on [`Ui::scroll`](crate::Ui::scroll), or
/// per area through [`ScrollOptions::config`](crate::ScrollOptions::config).
///
/// Two things ignore it, because they are direct manipulation rather than a
/// signal to interpret: dragging a scrollbar thumb, and a finger on the glass
/// (with the fling it throws). Those always track exactly.
///
/// ```ignore
/// ui.scroll.continuous = Smoothing::Eased { rate: 30.0 };  // ease everything
/// ui.scroll.line = 3.0 * row_height;                       // three rows a notch
///
/// // Or per area: a timeline need not feel like an inspector.
/// let opts = ScrollOptions {
///     config: Some(ScrollConfig { stepped: Smoothing::Instant, ..ui.scroll }),
///     ..ScrollOptions::horizontal(Size::Grow(1.0), Size::Fixed(28.0))
/// };
/// ```
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollConfig {
    /// Logical px one [`WheelUnit::Line`](crate::WheelUnit::Line) is worth.
    pub line: f32,
    /// Logical px one [`WheelUnit::Page`](crate::WheelUnit::Page) is worth.
    pub page: f32,
    /// Smoothing for a continuous signal (`WheelUnit::Pixel`). `Instant` by
    /// default, because the host is already sending a smooth stream.
    pub continuous: Smoothing,
    /// Smoothing for a stepped signal (`Line`, `Page`). Eased by default: a
    /// notch is a jump, and a jump has to be turned into motion somewhere.
    pub stepped: Smoothing,
    /// Fling deceleration after a touch drag (1/s): higher stops sooner.
    pub friction: f32,
    /// A fling below this speed (px/s) has stopped.
    pub fling_cutoff: f32,
}

impl Default for ScrollConfig {
    fn default() -> Self {
        Self {
            line: 24.0,
            page: 480.0,
            continuous: Smoothing::Instant,
            stepped: Smoothing::Eased { rate: 20.0 },
            friction: 3.2,
            fling_cutoff: 5.0,
        }
    }
}

impl ScrollConfig {
    /// Stepped units converted to logical px.
    pub(crate) fn steps_to_px(&self, lines: f32, pages: f32) -> f32 {
        lines * self.line + pages * self.page
    }
}
