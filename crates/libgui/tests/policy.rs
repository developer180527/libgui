//! Policy the **app** owns, not the library.
//!
//! `boundaries.rs` guards the structural half — no I/O, no clock, no globals,
//! no `target_os`. This guards the other half, which is quieter and erodes the
//! same way: a default baked in as a private constant, so the app can read the
//! library's taste but not replace it.
//!
//! Each of these was a fixed constant until an audit asked who owned it.

use libgui::*;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const INFO: FrameInfo = FrameInfo { screen_size: Vec2::new(600.0, 400.0), scale: 1.0, dt: 1.0 / 60.0 };

/// Tab width is an app's preference, and often a per-language one: Go is
/// eight, plenty of web repositories are two. It was four, full stop.
#[test]
fn an_app_sets_its_own_tab_width() {
    let ui = Ui::new(Theme::dark(), FONT).expect("font");
    let size = 16.0;
    // Measured against each other rather than against a space: `measure`
    // rounds a width up to a whole pixel, so four times a ceiled space is not
    // the same number as a ceiled four-space tab.
    let width_of_tab = |ui: &Ui| ui.fonts.measure(ui.font, size, "\t").x;

    assert_eq!(ui.fonts.tab_width(), DEFAULT_TAB_WIDTH);
    let four = width_of_tab(&ui);

    ui.fonts.set_tab_width(8);
    assert_eq!(ui.fonts.tab_width(), 8);
    let eight = width_of_tab(&ui);
    assert!((eight - four * 2.0).abs() <= 1.0, "eight spaces is not twice four: {eight} vs {four}");

    ui.fonts.set_tab_width(2);
    let two = width_of_tab(&ui);
    assert!((two * 2.0 - four).abs() <= 1.0, "two spaces is not half of four: {two} vs {four}");
}

/// The *caches* have to follow, or the first measurement of a string wins
/// forever. A shaped run holds the tab's advance, and so does a wrapped line.
#[test]
fn changing_the_tab_width_drops_the_caches_that_baked_it_in() {
    let ui = Ui::new(Theme::dark(), FONT).expect("font");
    let size = 16.0;
    // Measure first, so the run and wrap caches both hold the old advance.
    let before = ui.fonts.measure(ui.font, size, "\tx").x;
    let wrapped_before = ui.fonts.measure_wrapped(ui.font, size, "\tx", 1000.0).x;

    ui.fonts.set_tab_width(16);
    let after = ui.fonts.measure(ui.font, size, "\tx").x;
    let wrapped_after = ui.fonts.measure_wrapped(ui.font, size, "\tx", 1000.0).x;

    assert!(after > before, "the run cache served a stale tab advance: {after} vs {before}");
    assert!(wrapped_after > wrapped_before, "the wrap cache served a stale tab advance");
}

/// A zero-width tab would stack every character after it in one place.
#[test]
fn a_tab_is_never_zero_wide() {
    let ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.fonts.set_tab_width(0);
    assert_eq!(ui.fonts.tab_width(), 1);
    assert!(ui.fonts.measure(ui.font, 16.0, "\t").x > 0.0);
}

/// How long a pause closes an undo step is taste. Two seconds is the usual
/// figure and stays the default; an app that wants a step per keystroke, or
/// one that never breaks on time alone, can say so.
#[test]
fn an_app_sets_its_own_undo_grouping_pause() {
    /// Type `a`, let `gap_frames` quarter-seconds pass, type `b`, then count
    /// the undo steps it takes to empty the field.
    ///
    /// A quarter second per frame because `begin_frame` clamps `dt` to 0.25 —
    /// a stall must not make an animation jump — so the wait cannot be one
    /// long frame.
    fn steps_for(pause: f64, gap_frames: usize) -> usize {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        let mut binds = KeyBindings::new();
        binds.bind(Shortcut::plain(Key::Z).logo(), UiAction::Undo);
        ui.set_key_bindings(binds);
        ui.undo_run_pause = pause;
        let mut text = String::new();

        let frame = |ui: &mut Ui, text: &mut String, dt: f32| {
            ui.begin_frame(FrameInfo { dt, ..INFO });
            let col = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0));
            ui.container_id(Id::new("root"), col, Frame::none(), |ui| {
                ui.text_input("f", text, "");
            });
            let _ = ui.end_frame();
        };
        frame(&mut ui, &mut text, 0.0);
        // Focus the field.
        ui.push(InputEvent::PointerMoved { pos: Vec2::new(60.0, 20.0) });
        frame(&mut ui, &mut text, 0.0);
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: true });
        frame(&mut ui, &mut text, 0.0);
        ui.push(InputEvent::PointerButton { button: PointerButton::Primary, pressed: false });
        frame(&mut ui, &mut text, 0.0);

        ui.push(InputEvent::Text("a".into()));
        frame(&mut ui, &mut text, 0.0);
        // Let the clock run before the second keystroke.
        for _ in 0..gap_frames {
            frame(&mut ui, &mut text, 0.25);
        }
        ui.push(InputEvent::Text("b".into()));
        frame(&mut ui, &mut text, 0.0);
        assert_eq!(text, "ab", "the field did not take the typing");

        let mut n = 0;
        while !text.is_empty() && n < 10 {
            // The modifier comes from `ModifiersChanged`, not from the key
            // event: pressing SuperLeft as a key does not set `logo`.
            ui.push(InputEvent::ModifiersChanged(Modifiers { logo: true, ..Modifiers::NONE }));
            ui.push(InputEvent::Key { key: Key::Z, pressed: true, repeat: false });
            frame(&mut ui, &mut text, 0.0);
            ui.push(InputEvent::Key { key: Key::Z, pressed: false, repeat: false });
            ui.push(InputEvent::ModifiersChanged(Modifiers::NONE));
            frame(&mut ui, &mut text, 0.0);
            n += 1;
        }
        n
    }

    // A one-second gap under the two-second default is one run: one undo.
    assert_eq!(steps_for(2.0, 4), 1, "a pause inside the window split the run");
    // The same gap with a half-second pause is two runs: two undos.
    assert_eq!(steps_for(0.5, 4), 2, "a pause past the window did not close the run");
}

/// The atlas cap was a public field with no `&mut` path to it: `Fonts::atlas`
/// hands out a shared reference, so 4096 was unreachable. A UI with CJK
/// fallbacks at several sizes needs more, and past the cap the atlas resets
/// every frame instead of growing.
#[test]
fn an_app_sets_its_own_atlas_limit() {
    let ui = Ui::new(Theme::dark(), FONT).expect("font");
    assert_eq!(ui.fonts.atlas().max_size, 4096);

    ui.fonts.set_atlas_limit(8192);
    assert_eq!(ui.fonts.atlas().max_size, 8192);

    // Rounded up, because the atlas grows by doubling.
    ui.fonts.set_atlas_limit(5000);
    assert_eq!(ui.fonts.atlas().max_size, 8192);

    // And never under what is already allocated, which could not be honoured.
    ui.fonts.set_atlas_limit(16);
    assert!(ui.fonts.atlas().max_size >= ui.fonts.atlas().size.min(2048));
}

/// The cap is load-bearing, not decorative: it is what decides whether a glyph
/// too big for the atlas is rasterised at all.
#[test]
fn the_atlas_limit_actually_bounds_the_atlas() {
    /// Ask for one enormous glyph over a few frames and say whether it was
    /// ever rasterised and placed.
    ///
    /// Rasterising is the thing the cap governs: a glyph larger than the cap
    /// is skipped before the rasteriser is called at all — deliberately, since
    /// fontdue indexes its coverage buffer with an `i32`. Whether the result
    /// then lands on a small screen is a question about clipping, not about
    /// the atlas, so this counts glyphs and not quads.
    fn ever_rasterized(limit: Option<u32>) -> bool {
        let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
        if let Some(l) = limit {
            ui.fonts.set_atlas_limit(l);
        }
        for _ in 0..8 {
            ui.begin_frame(INFO);
            ui.text_with("W", 5000.0, Color::WHITE);
            let _ = ui.end_frame();
            if ui.frame_cost().glyphs_rasterized > 0 {
                return true;
            }
        }
        false
    }

    // 5000 px is past the default 4096 cap: never rasterised, however many
    // frames it gets. Its advance still counts, so layout does not move.
    assert!(!ever_rasterized(None), "a glyph past the default cap was rasterised anyway");
    // Raise the cap and the same glyph is rasterised and placed.
    assert!(ever_rasterized(Some(8192)), "a glyph inside the raised cap was still skipped");
}

/// Raising the cap has to actually reach the glyph that motivated raising it.
///
/// A glyph too big for the atlas *as it stands* used to be recorded as
/// unplaceable and never reconsidered, because the check asked whether it fit
/// the current size rather than whether growing could hold it. So the limit
/// was settable and inert: the atlas sat at 2048 while the cap said 8192.
#[test]
fn a_glyph_too_big_for_today_s_atlas_asks_it_to_grow() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let start = ui.fonts.atlas().size;
    ui.fonts.set_atlas_limit(8192);
    for _ in 0..8 {
        ui.begin_frame(INFO);
        ui.text_with("W", 5000.0, Color::WHITE);
        let _ = ui.end_frame();
    }
    let grown = ui.fonts.atlas().size;
    assert!(grown > start, "the atlas never grew: {start} -> {grown}");
    assert!(grown <= 8192, "the atlas grew past its limit: {grown}");
}
