//! Scopes and meters: what they cost, what they report, and how a meter keeps
//! time.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const SCREEN: Vec2 = Vec2::new(640.0, 400.0);

fn ui() -> Ui {
    Ui::new(Theme::dark(), FONT).expect("font")
}

fn frame<R>(ui: &mut Ui, dt: f32, build: impl FnOnce(&mut Ui) -> R) -> (R, FrameOutput<'_>) {
    ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt });
    let r = build(ui);
    (r, ui.end_frame())
}

/// A million samples and a thousand draw the same number of instances: the
/// cost follows the scope's width.
#[test]
fn a_scope_costs_what_its_width_does() {
    let count = |n: usize| {
        let s: Vec<f32> = (0..n).map(|i| (i as f32 * 0.001).sin()).collect();
        let mut ui = ui();
        let mut instances = 0;
        for _ in 0..3 {
            let (_, out) = frame(&mut ui, 1.0 / 60.0, |ui| {
                ui.scope("wave", &[ScopeTrace::new(&s[..])], &ScopeOptions { range: Some((-1.0, 1.0)), ..Default::default() })
            });
            instances = out.draw.instances.len();
        }
        instances
    };
    let (small, huge) = (count(5_000), count(1_000_000));
    assert!(huge <= small + 8, "a million samples drew {huge} instances against {small} for five thousand");
    assert!(huge < 2 * SCREEN.x as usize, "{huge} instances for a {} px wide scope", SCREEN.x);
}

/// A trace shorter than the scope is the line through its samples: one
/// segment per gap between samples. (A long diagonal is drawn as several
/// strips of the same segment, so segments are counted by their endpoints.)
#[test]
fn a_short_trace_is_a_line_through_its_points() {
    let s = [0.0f32, 1.0, 0.5, 0.75];
    let mut ui = ui();
    let mut lines = 0;
    for _ in 0..2 {
        let (_, out) = frame(&mut ui, 1.0 / 60.0, |ui| {
            ui.scope("few", &[ScopeTrace::new(&s[..])], &ScopeOptions { grid: (0, 0), range: Some((0.0, 1.0)), ..Default::default() })
        });
        let mut ends: Vec<[u32; 4]> = out
            .draw
            .instances
            .iter()
            .filter(|i| i.params[3] == render_contract::PrimitiveKind::Line.code())
            .map(|i| i.uv.map(f32::to_bits))
            .collect();
        ends.sort();
        ends.dedup();
        lines = ends.len();
    }
    assert_eq!(lines, 3);
}

/// The fitted range covers the data, and the pointer reads the sample under
/// it — from a ring, oldest first.
#[test]
fn the_range_fits_and_the_readout_finds_the_sample() {
    // A ring whose oldest sample is at index 3: logically 0, 10, 20, ... 90.
    let buf = [70.0, 80.0, 90.0, 0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0];
    let mut ui = ui();
    let mut r = None;
    for k in 0..3 {
        let (resp, _) = frame(&mut ui, 1.0 / 60.0, |ui| ui.scope("ring", &[ScopeTrace::new(Trace::ring(&buf, 3)).label("v")], &ScopeOptions::default()));
        r = Some(resp);
        if k == 1 {
            // Rest the pointer three tenths of the way across.
            let rr = resp.response.rect;
            ui.push(InputEvent::PointerMoved { pos: Vec2::new(rr.x + rr.w * 0.3, rr.center().y) });
        }
    }
    let r = r.unwrap();
    assert!(r.range.0 <= 0.0 && r.range.1 >= 90.0, "the fitted range {:?} does not hold the data", r.range);
    assert_eq!(r.index(buf.len()), Some(3), "the readout is not at the sample three tenths across");
    // Which, read oldest first through the ring, is 30.
    assert_eq!(Trace::ring(&buf, 3).get(r.index(buf.len()).unwrap()), 30.0);
}

/// A peak is held for `hold` seconds, then falls to the value — and while it
/// waits the window may sleep until it is due.
#[test]
fn a_meter_holds_its_peak_then_lets_it_fall() {
    let opts = MeterOptions { hold: 1.0, fall: 1.0, ..Default::default() };
    let mut ui = ui();
    let meter = |ui: &mut Ui, v: f32, dt: f32| {
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt });
        let r = ui.meter("level", v, &opts);
        let wake = ui.end_frame().platform.repaint_after;
        (r, wake)
    };
    meter(&mut ui, 0.9, 1.0 / 60.0);
    let (r, wake) = meter(&mut ui, 0.1, 0.25);
    assert_eq!(r.peak, 0.9, "the peak did not hold");
    let wake = wake.expect("a held peak let the window sleep forever");
    assert!(wake > 0.5 && wake <= 0.75 + 1e-4, "asked to wake in {wake} s, not when the hold ends");
    // Sleep until then: the hold is over, and the peak starts to fall.
    let (r, wake) = meter(&mut ui, 0.1, wake + 0.1);
    assert!(r.peak < 0.9 && r.peak > 0.1, "the peak did not start falling: {}", r.peak);
    assert_eq!(wake, Some(0.0), "a falling peak did not ask for frames");
    for _ in 0..120 {
        meter(&mut ui, 0.1, 1.0 / 60.0);
    }
    let (r, wake) = meter(&mut ui, 0.1, 1.0 / 60.0);
    assert_eq!(r.peak, 0.1, "the peak never came down to the value");
    assert_eq!(wake, None, "a meter at rest kept the window awake");
}

/// The over light latches at the top of the scale and stays lit after the
/// value comes down, until the meter is clicked.
#[test]
fn the_clip_light_latches_until_clicked() {
    let opts = MeterOptions { clip_light: true, ..Default::default() };
    let mut ui = ui();
    let step = |ui: &mut Ui, v: f32| {
        ui.begin_frame(FrameInfo { screen_size: SCREEN, scale: 1.0, dt: 1.0 / 60.0 });
        let r = ui.meter("level", v, &opts);
        drop(ui.end_frame());
        r
    };
    assert!(!step(&mut ui, 0.5).clipped);
    assert!(step(&mut ui, 1.2).clipped, "going over the top did not light it");
    let r = step(&mut ui, 0.2);
    assert!(r.clipped, "the light went out on its own");
    let c = r.response.rect.center();
    ui.push(InputEvent::PointerMoved { pos: c });
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
    step(&mut ui, 0.2);
    ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
    let r = step(&mut ui, 0.2);
    assert!(!r.clipped, "a click did not clear the light");
    assert_eq!(r.peak, 0.2, "a click did not reset the peak");
}

/// The zones colour the bar from the theme: lit up into the over zone, the
/// meter shows the danger colour; below the warning zone it does not.
#[test]
fn the_zones_take_the_themes_colours() {
    let opts = MeterOptions { zones: Some((0.6, 0.9)), hold: 0.0, ..Default::default() };
    let danger = Theme::dark().palette.danger.to_array();
    let lit = |v: f32| {
        let mut ui = ui();
        let mut any = false;
        for _ in 0..2 {
            let (_, out) = frame(&mut ui, 1.0 / 60.0, |ui| ui.meter("m", v, &opts));
            any = out.draw.instances.iter().any(|i| i.color == danger);
        }
        any
    };
    assert!(lit(0.95), "a value in the over zone was not drawn in the danger colour");
    assert!(!lit(0.5), "a value below the warning zone was drawn in the danger colour");
}
