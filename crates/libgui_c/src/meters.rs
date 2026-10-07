//! Traces, scopes and level meters.

use crate::containers::LibguiPainter;
use crate::convert::str_or_empty;
use crate::handle::{with_ui, LibguiUi};
use crate::types::{LibguiColor, LibguiRect, LibguiResponse};
use libgui::{Color, MeterOptions, Rect, ScopeOptions, ScopeTrace, Size, Trace};
use std::os::raw::c_char;

fn color(c: LibguiColor) -> Color {
    Color::rgba(c.r, c.g, c.b, c.a)
}

/// `count` samples at `samples`, oldest at `start`: a borrowed [`Trace`].
/// Null or empty is an empty trace.
unsafe fn trace<'a>(samples: *const f32, count: u64, start: u64) -> Trace<'a> {
    if samples.is_null() || count == 0 {
        return Trace::new(&[]);
    }
    Trace::ring(unsafe { std::slice::from_raw_parts(samples, count as usize) }, start as usize)
}

/// `count` samples drawn across `r`, `lo` at the bottom and `hi` at the top,
/// as a line `width` wide; `start` is the oldest sample's index in a ring
/// buffer (0 for a plain array). One stroke per pixel column at most, however
/// many samples. Non-finite samples are gaps.
///
/// # Safety
/// `p` null or the painter handed to a paint callback; `samples` null or
/// `count` floats.
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn libgui_painter_trace(
    p: *mut LibguiPainter,
    r: LibguiRect,
    samples: *const f32,
    count: u64,
    start: u64,
    lo: f32,
    hi: f32,
    width: f32,
    c: LibguiColor,
) {
    let t = unsafe { trace(samples, count, start) };
    if let Some(p) = crate::containers::painter(p) {
        p.trace(Rect::new(r.x, r.y, r.w, r.h), t, (lo, hi), width, color(c));
    }
}

/// The area between the samples and `baseline`, filled, one rect per pixel
/// column.
///
/// # Safety
/// As [`libgui_painter_trace`].
#[no_mangle]
#[allow(clippy::too_many_arguments)]
pub unsafe extern "C" fn libgui_painter_trace_fill(
    p: *mut LibguiPainter,
    r: LibguiRect,
    samples: *const f32,
    count: u64,
    start: u64,
    lo: f32,
    hi: f32,
    baseline: f32,
    c: LibguiColor,
) {
    let t = unsafe { trace(samples, count, start) };
    if let Some(p) = crate::containers::painter(p) {
        p.trace_fill(Rect::new(r.x, r.y, r.w, r.h), t, (lo, hi), baseline, color(c));
    }
}

/// One trace of a [`libgui_scope`]. Mirrors [`libgui::ScopeTrace`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiScopeTrace {
    pub samples: *const f32,
    pub count: u64,
    /// The oldest sample's index, for a ring buffer; 0 for a plain array.
    pub start: u64,
    pub color: LibguiColor,
    /// 0: take the theme's next trace colour and ignore `color`.
    pub has_color: u8,
    /// Fill between the trace and the scope's baseline.
    pub fill: u8,
    pub _pad: [u8; 2],
    pub width: f32,
    /// Shown in the readout; may be null.
    pub label: *const c_char,
}

/// Mirrors [`libgui::ScopeOptions`]. Start from [`libgui_scope_options_default`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiScopeOptions {
    pub height: f32,
    /// 1: fit the traces' own values, and ignore `lo` and `hi`.
    pub auto_range: u8,
    /// Show each trace's value under the pointer.
    pub readout: u8,
    pub _pad: [u8; 2],
    pub lo: f32,
    pub hi: f32,
    /// Grid divisions across and up; zero for none.
    pub grid_x: u32,
    pub grid_y: u32,
    pub baseline: f32,
}

/// # Safety
/// `out` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_scope_options_default(out: *mut LibguiScopeOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        let d = ScopeOptions::default();
        *o = LibguiScopeOptions {
            height: d.height,
            auto_range: 1,
            readout: d.readout as u8,
            _pad: [0; 2],
            lo: 0.0,
            hi: 1.0,
            grid_x: d.grid.0,
            grid_y: d.grid.1,
            baseline: d.baseline,
        };
    }
}

/// Mirrors [`libgui::ScopeResponse`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiScopeResponse {
    pub response: LibguiResponse,
    /// The range drawn: the one given, or the one fitted.
    pub lo: f32,
    pub hi: f32,
    /// Where the pointer is across the scope, 0 at the oldest sample and 1 at
    /// the newest; negative when it is not over it.
    pub at: f32,
    pub _pad: u32,
}

/// A scope: `count` traces over a grid, newest sample at the right, with a
/// readout under the pointer. The samples are reduced to one min/max pair per
/// pixel column while the frame is built, so a trace of any length costs
/// about a screen width; nothing is retained after the call. `opts` may be
/// null.
///
/// # Safety
/// `ui` null or live; `key` a string; `traces` null or `count` valid traces,
/// each `samples` null or `count` floats and `label` null or a string.
#[no_mangle]
pub unsafe extern "C" fn libgui_scope(
    ui: *mut LibguiUi,
    key: *const c_char,
    traces: *const LibguiScopeTrace,
    count: u64,
    opts: *const LibguiScopeOptions,
) -> LibguiScopeResponse {
    let key = unsafe { str_or_empty(key, "libgui_scope") };
    let given: &[LibguiScopeTrace] = if traces.is_null() || count == 0 { &[] } else { unsafe { std::slice::from_raw_parts(traces, count as usize) } };
    let list: Vec<ScopeTrace> = given
        .iter()
        .map(|t| {
            let label = (!t.label.is_null()).then(|| unsafe { str_or_empty(t.label, "libgui_scope") });
            ScopeTrace {
                trace: unsafe { trace(t.samples, t.count, t.start) },
                color: (t.has_color != 0).then(|| color(t.color)),
                width: t.width,
                fill: t.fill != 0,
                label,
            }
        })
        .collect();
    let o = match unsafe { opts.as_ref() } {
        Some(o) => ScopeOptions {
            height: o.height,
            range: (o.auto_range == 0).then_some((o.lo, o.hi)),
            grid: (o.grid_x, o.grid_y),
            baseline: o.baseline,
            readout: o.readout != 0,
        },
        None => ScopeOptions::default(),
    };
    with_ui(ui, LibguiScopeResponse { at: -1.0, ..Default::default() }, |u| {
        let r = u.scope(key, &list, &o);
        LibguiScopeResponse { response: r.response.into(), lo: r.range.0, hi: r.range.1, at: r.at.unwrap_or(-1.0), _pad: 0 }
    })
}

/// Mirrors [`libgui::MeterOptions`]. Start from [`libgui_meter_options_default`]
/// or [`libgui_meter_options_audio_db`].
#[repr(C)]
#[derive(Clone, Copy, Debug)]
pub struct LibguiMeterOptions {
    /// The bottom and top of the scale, in the app's units.
    pub lo: f32,
    pub hi: f32,
    /// Where the warning and over zones start; used when `has_zones` is 1.
    pub warn: f32,
    pub over: f32,
    pub has_zones: u8,
    /// 1: fills bottom to top; 0: left to right.
    pub vertical: u8,
    pub clip_light: u8,
    pub _pad: u8,
    /// Along the bar: 0 fixed (`length` px), 1 fit, 2 grow (weight `length`).
    pub length_kind: u32,
    pub length: f32,
    pub thickness: f32,
    /// Seconds a peak is held; 0 for no peak tick.
    pub hold: f32,
    /// How fast a held peak falls, in ranges per second.
    pub fall: f32,
    /// An LED ladder of this many cells; 0 for a solid bar.
    pub cells: u32,
    /// A tick every this many units; 0 for none.
    pub tick_step: f32,
}

impl From<MeterOptions> for LibguiMeterOptions {
    fn from(o: MeterOptions) -> Self {
        let (length_kind, length) = match o.length {
            Size::Fixed(v) => (0, v),
            Size::Fit => (1, 0.0),
            Size::Grow(w) => (2, w),
        };
        let (warn, over) = o.zones.unwrap_or((0.0, 0.0));
        Self {
            lo: o.range.0,
            hi: o.range.1,
            warn,
            over,
            has_zones: o.zones.is_some() as u8,
            vertical: (o.axis == libgui::Axis::Y) as u8,
            clip_light: o.clip_light as u8,
            _pad: 0,
            length_kind,
            length,
            thickness: o.thickness,
            hold: o.hold,
            fall: o.fall,
            cells: o.cells,
            tick_step: o.tick_step.unwrap_or(0.0),
        }
    }
}

impl From<LibguiMeterOptions> for MeterOptions {
    fn from(o: LibguiMeterOptions) -> Self {
        // A plain number from C, not an enum: anything unknown grows.
        let length = match o.length_kind {
            0 => Size::Fixed(o.length),
            1 => Size::Fit,
            _ => Size::Grow(if o.length > 0.0 { o.length } else { 1.0 }),
        };
        Self {
            range: (o.lo, o.hi),
            zones: (o.has_zones != 0).then_some((o.warn, o.over)),
            axis: if o.vertical != 0 { libgui::Axis::Y } else { libgui::Axis::X },
            length,
            thickness: o.thickness,
            hold: o.hold,
            fall: o.fall,
            cells: o.cells,
            tick_step: (o.tick_step > 0.0).then_some(o.tick_step),
            clip_light: o.clip_light != 0,
        }
    }
}

/// # Safety
/// `out` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_meter_options_default(out: *mut LibguiMeterOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        *o = MeterOptions::default().into();
    }
}

/// An audio channel in dBFS: −60 to 0, warning from −18, over from −6,
/// vertical, an LED ladder, a clip light.
///
/// # Safety
/// `out` null or writable.
#[no_mangle]
pub unsafe extern "C" fn libgui_meter_options_audio_db(out: *mut LibguiMeterOptions) {
    if let Some(o) = unsafe { out.as_mut() } {
        *o = MeterOptions::audio_db().into();
    }
}

/// Mirrors [`libgui::MeterResponse`].
#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
pub struct LibguiMeterResponse {
    pub response: LibguiResponse,
    /// The held peak, in the app's units.
    pub peak: f32,
    /// Over the top since last cleared; a click on the meter clears it.
    pub clipped: u8,
    pub _pad: [u8; 3],
}

unsafe fn meter(ui: *mut LibguiUi, key: *const c_char, value: f32, average: Option<f32>, opts: *const LibguiMeterOptions, who: &str) -> LibguiMeterResponse {
    let key = unsafe { str_or_empty(key, who) };
    let o = unsafe { opts.as_ref() }.map(|o| MeterOptions::from(*o)).unwrap_or_default();
    with_ui(ui, LibguiMeterResponse::default(), |u| {
        let r = match average {
            Some(a) => u.meter_with_average(key, value, a, &o),
            None => u.meter(key, value, &o),
        };
        LibguiMeterResponse { response: r.response.into(), peak: r.peak, clipped: r.clipped as u8, _pad: [0; 3] }
    })
}

/// A level meter showing `value`, with a held peak and an over-range light
/// that libgui keeps between frames. `opts` may be null.
///
/// # Safety
/// `ui` null or live; `key` a string; `opts` null or valid.
#[no_mangle]
pub unsafe extern "C" fn libgui_meter(ui: *mut LibguiUi, key: *const c_char, value: f32, opts: *const LibguiMeterOptions) -> LibguiMeterResponse {
    unsafe { meter(ui, key, value, None, opts, "libgui_meter") }
}

/// [`libgui_meter`] with an average (RMS under peak) drawn solid under the
/// instantaneous value.
///
/// # Safety
/// As [`libgui_meter`].
#[no_mangle]
pub unsafe extern "C" fn libgui_meter_with_average(
    ui: *mut LibguiUi,
    key: *const c_char,
    value: f32,
    average: f32,
    opts: *const LibguiMeterOptions,
) -> LibguiMeterResponse {
    unsafe { meter(ui, key, value, Some(average), opts, "libgui_meter_with_average") }
}
