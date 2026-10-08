//! The glyph atlas never loses a glyph on screen to being full (WO-1).
//!
//! Two `Ui`s draw identical frames: one on the default budget, one with room
//! for everything. The text in use slides through ~140 sizes and three
//! scripts, far more than the budget holds over the run, though any one frame
//! fits. The capped one must draw every glyph the uncapped one draws, on every
//! frame, and the same pixels (to within a level of rounding); and it must
//! have reused pages to do it.

use libgui::*;
use libgui_soft::SoftRenderer;

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");
const SIZE: Vec2 = Vec2::new(900.0, 700.0);

const LINES: [&str; 3] = [
    "Sphinx of black quartz, judge my vow 0123456789",
    "Ξεσκεπάζω την ψυχοφθόρα βδελυγμία",
    "Съешь же ещё этих мягких французских булок",
];

fn sizes(frame: usize) -> [f32; 4] {
    let base = frame / 15;
    std::array::from_fn(|k| 8.0 + ((base * 7 + k * 37) % 140) as f32)
}

fn build(ui: &mut Ui, frame: usize) {
    for s in sizes(frame) {
        for line in LINES {
            ui.text_with(line, s, Color::WHITE);
        }
    }
}

fn glyphs(out: &FrameOutput) -> usize {
    out.draw.instances.iter().filter(|i| i.params[3] == render_contract::PrimitiveKind::Glyph.code()).count()
}

#[test]
fn a_full_atlas_reuses_pages_and_never_drops_a_glyph() {
    let mut capped = Ui::new(Theme::dark(), FONT).expect("font");
    let mut roomy = Ui::new(Theme::dark(), FONT).expect("font");
    roomy.fonts.set_atlas_limit(16384);
    let info = FrameInfo { screen_size: SIZE, scale: 1.0, dt: 1.0 / 60.0 };
    let (mut evictions, mut checked) = (0u32, 0);
    for f in 0..600 {
        capped.begin_frame(info);
        build(&mut capped, f);
        let a = capped.end_frame();
        let ga = glyphs(&a);
        let pixels_a = (f % 60 == 59).then(|| SoftRenderer::new().render_to_image(&a, SIZE.x as u32, SIZE.y as u32));
        drop(a);
        let cost = capped.frame_cost();
        roomy.begin_frame(info);
        build(&mut roomy, f);
        let b = roomy.end_frame();
        assert_eq!(ga, glyphs(&b), "frame {f}: the capped atlas drew {ga} glyphs, the roomy one {}", glyphs(&b));
        assert_eq!(cost.atlas_overflows, 0, "frame {f}: a glyph did not fit, though the frame fits the budget");
        evictions += cost.atlas_evictions;
        if let Some(pa) = pixels_a {
            let pb = SoftRenderer::new().render_to_image(&b, SIZE.x as u32, SIZE.y as u32);
            // Within one level: a glyph at another place in the atlas is
            // sampled through slightly different rounding, as on a GPU. A
            // missing or wrong glyph is off by tens or hundreds.
            let worst = pa.data.iter().zip(&pb.data).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0);
            assert!(worst <= 1, "frame {f}: the capped atlas drew different pixels (off by {worst})");
            checked += 1;
        }
    }
    assert_eq!(checked, 10);
    assert!(evictions > 0, "the run never filled the budget, so it proved nothing");
    let atlas = capped.fonts.atlas();
    assert!(atlas.texels() <= 4096 * 4096, "the atlas went past its budget: {} texels", atlas.texels());
    assert!(roomy.fonts.atlas().texels() > atlas.texels(), "the roomy atlas held no more than the capped one");
}

/// A steady frame on a multi-page atlas rasterises nothing and evicts nothing.
#[test]
fn a_settled_frame_on_several_pages_costs_nothing() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    // A budget the working set fits in, on several pages.
    ui.fonts.set_atlas_limit(8192);
    // Tall enough that nothing is culled: text off screen is never rasterised.
    let info = FrameInfo { screen_size: Vec2::new(4000.0, 4000.0), scale: 1.0, dt: 1.0 / 60.0 };
    let draw = |ui: &mut Ui| {
        for s in (40..=300).step_by(20).map(|s| s as f32) {
            for line in LINES {
                ui.text_with(line, s, Color::WHITE);
            }
        }
    };
    for _ in 0..3 {
        ui.begin_frame(info);
        draw(&mut ui);
        drop(ui.end_frame());
    }
    assert!(ui.fonts.atlas().pages.len() > 1, "the test wanted more than one page in use");
    ui.begin_frame(info);
    draw(&mut ui);
    let out = ui.end_frame();
    let pages: std::collections::BTreeSet<u32> = out
        .draw
        .batches
        .iter()
        .filter_map(|b| match b.texture {
            TextureId::Atlas(p) => Some(p),
            _ => None,
        })
        .collect();
    drop(out);
    let cost = ui.frame_cost();
    assert_eq!((cost.glyphs_rasterized, cost.atlas_evictions, cost.atlas_overflows), (0, 0, 0));
    assert!(pages.len() > 1, "glyphs on several pages, but every batch named one");
}

/// A page drawn from this frame is never reused during it — including when
/// what was drawn came from the cache rather than being placed this frame.
/// One page of budget: a line is drawn (from the cache, it was placed last
/// frame), then the frame asks for more than the page holds. The overflow is
/// counted and waits for the next frame; the line already drawn keeps its
/// pixels instead of pointing into a page emptied under it.
#[test]
fn a_page_in_use_this_frame_is_never_reused_during_it() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.fonts.set_atlas_limit(2048);
    let info = FrameInfo { screen_size: Vec2::new(1600.0, 1200.0), scale: 1.0, dt: 1.0 / 60.0 };
    let frame = |ui: &mut Ui, flood: bool| -> (Vec<u8>, testing::FrameCost) {
        ui.begin_frame(info);
        ui.text_with("Reference line, drawn first", 28.0, Color::WHITE);
        if flood {
            for s in (30..300).step_by(9) {
                ui.text_with(LINES[0], s as f32, Color::WHITE);
            }
        }
        let out = ui.end_frame();
        let img = SoftRenderer::new().render_to_image(&out, 1600, 1200);
        drop(out);
        // The reference line's strip, top of the window.
        (img.data[..1600 * 4 * 40].to_vec(), ui.frame_cost())
    };
    let (clean, _) = frame(&mut ui, false);
    frame(&mut ui, false);
    let (flooded, cost) = frame(&mut ui, true);
    assert!(cost.atlas_overflows > 0, "the flood fit in one page, so the test proved nothing");
    assert!(flooded == clean, "the line drawn first lost its pixels when the page under it was reused");
}

/// The same protection when the page is only *read* this frame. The line
/// comes entirely from the cache, so nothing is placed on its page; then one
/// glyph so large its page would need the whole budget asks for room. Page 0
/// is in use by the line and must not be released for it.
#[test]
fn a_page_read_from_the_cache_this_frame_is_in_use() {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let info = FrameInfo { screen_size: Vec2::new(1600.0, 1200.0), scale: 1.0, dt: 1.0 / 60.0 };
    let frame = |ui: &mut Ui, giant: bool| -> (Vec<u8>, testing::FrameCost) {
        ui.begin_frame(info);
        ui.text_with("Reference line, from the cache", 28.0, Color::WHITE);
        if giant {
            ui.text_with("W", 3000.0, Color::WHITE);
        }
        let out = ui.end_frame();
        let img = SoftRenderer::new().render_to_image(&out, 1600, 1200);
        drop(out);
        (img.data[..1600 * 4 * 40].to_vec(), ui.frame_cost())
    };
    let (clean, _) = frame(&mut ui, false);
    frame(&mut ui, false);
    let (with_giant, cost) = frame(&mut ui, true);
    assert_eq!(cost.glyphs_rasterized, 1, "the line was not all from the cache");
    assert!(cost.atlas_overflows > 0, "the giant glyph found room, so the test proved nothing");
    assert!(with_giant == clean, "the line lost its pixels when the page it was read from was released");
}
