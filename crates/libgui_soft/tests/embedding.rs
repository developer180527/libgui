//! Showing another renderer's frames — the thing a game engine, a CAD kernel
//! or a video decoder actually needs from a UI library.
//!
//! The contract is in `libgui::render_contract`: the app registers its colour
//! target under a `TextureId::User(n)`, `ui.viewport` draws it, and the backend
//! binds whatever `n` means to it. Everything here drives that path for real
//! and checks the *pixels*, because every way of getting it wrong — a flipped
//! v axis, a half-texel offset, premultiplied alpha, the wrong size on a
//! high-DPI display — looks fine in a type signature.

use libgui::*;
use libgui_soft::{SoftRenderer, Target, Texture};

const FONT: &[u8] = include_bytes!("../../../assets/Inter.ttf");

/// A "rendered frame" from somebody else's renderer: four quadrants, so an
/// axis flip or a mirror is obvious, and an alpha of zero throughout, because
/// the contract says a user texture composites as opaque.
fn engine_frame(w: u32, h: u32) -> Texture {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let left = x < w / 2;
            let top = y < h / 2;
            let px: [u8; 4] = match (top, left) {
                (true, true) => [255, 0, 0, 0],    // top-left red
                (true, false) => [0, 255, 0, 0],   // top-right green
                (false, true) => [0, 0, 255, 0],   // bottom-left blue
                (false, false) => [255, 255, 0, 0], // bottom-right yellow
            };
            data.extend_from_slice(&px);
        }
    }
    Texture { width: w, height: h, data }
}

struct Shot {
    img: Target,
    /// Where the viewport landed, in logical px.
    rect: Rect,
    scale: f32,
}

impl Shot {
    /// The pixel at a fraction across the viewport's rect.
    fn at(&self, fx: f32, fy: f32) -> [u8; 4] {
        let x = (self.rect.x + self.rect.w * fx) * self.scale;
        let y = (self.rect.y + self.rect.h * fy) * self.scale;
        self.img.pixel(x as u32, y as u32)
    }
}

/// Build a frame with `body`, render it through the CPU backend with `soft`,
/// and report where the viewport ended up. `body` hands back the viewport's
/// own rect — asking the `Response`, which is how an app learns it.
fn run(soft: &mut SoftRenderer, scale: f32, body: impl Fn(&mut Ui) -> Rect + Copy) -> Shot {
    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let size = Vec2::new(400.0, 300.0);
    let info = FrameInfo { screen_size: size, scale, dt: 1.0 / 60.0 };
    for _ in 0..3 {
        ui.begin_frame(info);
        body(&mut ui);
        let _ = ui.end_frame();
    }
    ui.begin_frame(info);
    let rect = body(&mut ui);
    let out = ui.end_frame();
    let (w, h) = ((size.x * scale) as u32, (size.y * scale) as u32);
    let img = soft.render_to_image(&out, w, h);
    drop(out);
    Shot { img, rect, scale }
}

fn panel(ui: &mut Ui, tex: TextureId) -> Rect {
    let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
    ui.container(layout, Frame::none(), |ui| ui.viewport("scene", tex, |_, _| {}).rect)
}

/// The frame arrives the way up it was drawn, fills the rect it was given, and
/// is composited opaquely whatever its alpha says.
#[test]
fn a_registered_frame_is_shown_the_right_way_up() {
    let mut soft = SoftRenderer::new();
    let tex = soft.register_texture(engine_frame(64, 64));
    let shot = run(&mut soft, 1.0, move |ui| panel(ui, tex));

    assert!(shot.rect.w > 100.0 && shot.rect.h > 100.0, "the viewport got no room: {:?}", shot.rect);
    // Quadrants, well inside each corner so the rounded edge and border are
    // not what is being sampled.
    let rgb = |p: [u8; 4]| [p[0], p[1], p[2]];
    assert_eq!(rgb(shot.at(0.25, 0.25)), [255, 0, 0], "top-left is not the texture's top-left");
    assert_eq!(rgb(shot.at(0.75, 0.25)), [0, 255, 0], "top-right wrong — the u axis is mirrored");
    assert_eq!(rgb(shot.at(0.25, 0.75)), [0, 0, 255], "bottom-left wrong — the v axis is flipped");
    assert_eq!(rgb(shot.at(0.75, 0.75)), [255, 255, 0], "bottom-right wrong");
    // Alpha 0 in the source, opaque on screen: the contract says a user
    // texture's alpha is ignored, so an engine that renders premultiplied or
    // leaves garbage in alpha still composites.
    assert_eq!(shot.at(0.25, 0.25)[3], 255, "the frame was blended with its own alpha");
}

/// A high-DPI display: the texture the engine should render is bigger than the
/// rect, and `physical_px` is the number to size it to.
#[test]
fn physical_px_is_the_size_the_engine_should_render() {
    let mut soft = SoftRenderer::new();
    let tex = soft.register_texture(engine_frame(64, 64));
    let shot = run(&mut soft, 2.0, move |ui| panel(ui, tex));

    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    ui.begin_frame(FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 2.0, dt: 1.0 / 60.0 });
    let (w, h) = ui.physical_px(shot.rect);
    let _ = ui.end_frame();
    assert_eq!(w, (shot.rect.w * 2.0).round() as u32);
    assert_eq!(h, (shot.rect.h * 2.0).round() as u32);
    // And the picture still lands correctly at 2x.
    let rgb = |p: [u8; 4]| [p[0], p[1], p[2]];
    assert_eq!(rgb(shot.at(0.25, 0.25)), [255, 0, 0]);
    assert_eq!(rgb(shot.at(0.75, 0.75)), [255, 255, 0]);
}

/// A texture id the backend has never heard of — a target retired by a resize
/// mid-frame, a panel whose renderer has not started yet — is skipped, not a
/// panic and not a hole: everything around it still draws.
#[test]
fn an_unknown_texture_is_skipped_and_the_rest_still_draws() {
    let mut soft = SoftRenderer::new();
    let known = soft.register_texture(engine_frame(32, 32));
    let ghost = TextureId::User(4242);

    let with_ghost = run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame::none(), |ui| {
            let r = ui.viewport("scene", ghost, |_, _| {}).rect;
            let _ = ui.button("after the viewport");
            r
        })
    });
    // The button after it is still on screen: the batch for the missing
    // texture was skipped, not the rest of the frame.
    let lit = with_ghost.img.data.chunks(4).filter(|p| p[0] > 60).count();
    assert!(lit > 200, "the frame after an unknown texture is empty ({lit} lit pixels)");
    // And the viewport itself drew no picture.
    let rgb = |p: [u8; 4]| [p[0], p[1], p[2]];
    assert_ne!(rgb(with_ghost.at(0.25, 0.25)), [255, 0, 0]);

    // Registering the same id later makes it appear, which is the resize path.
    soft.update_texture(ghost, engine_frame(32, 32));
    let now = run(&mut soft, 1.0, move |ui| panel(ui, ghost));
    assert_eq!(rgb(now.at(0.25, 0.25)), [255, 0, 0], "the texture did not appear once registered");
    let _ = known;
}

/// Two renderers' frames in one UI frame: the batches switch texture and back,
/// and neither is drawn with the other's.
#[test]
fn two_viewports_show_their_own_frames() {
    let mut soft = SoftRenderer::new();
    let a = soft.register_texture(engine_frame(32, 32));
    let mut solid = engine_frame(32, 32);
    for px in solid.data.chunks_mut(4) {
        px.copy_from_slice(&[10, 10, 200, 0]);
    }
    let b = soft.register_texture(solid);

    let mut ui = Ui::new(Theme::dark(), FONT).expect("font");
    let info = FrameInfo { screen_size: Vec2::new(400.0, 300.0), scale: 1.0, dt: 1.0 / 60.0 };
    let build = |ui: &mut Ui| -> (Rect, Rect) {
        ui.container(Layout::row().width(Size::Grow(1.0)).height(Size::Grow(1.0)).gap(8.0), Frame::none(), |ui| {
            let l = ui.viewport("left", a, |_, _| {}).rect;
            let r = ui.viewport("right", b, |_, _| {}).rect;
            (l, r)
        })
    };
    for _ in 0..3 {
        ui.begin_frame(info);
        build(&mut ui);
        let _ = ui.end_frame();
    }
    ui.begin_frame(info);
    let (left, right) = build(&mut ui);
    let out = ui.end_frame();
    // Three or more batches: atlas chrome, texture A, texture B.
    let switches = out.draw.batches.len();
    let img = soft.render_to_image(&out, 400, 300);
    drop(out);
    assert!(switches >= 3, "two viewports did not split the batches: {switches}");

    let pick = |r: Rect, fx: f32| {
        let p = img.pixel((r.x + r.w * fx) as u32, (r.y + r.h * 0.25) as u32);
        [p[0], p[1], p[2]]
    };
    assert_eq!(pick(left, 0.25), [255, 0, 0], "the left viewport shows the wrong frame");
    assert_eq!(pick(right, 0.25), [10, 10, 200], "the right viewport shows the wrong frame");
}

/// An overlay draws *over* the frame — gizmo labels, a HUD, selection
/// rectangles — and is clipped to the viewport.
#[test]
fn an_overlay_draws_over_the_frame_and_is_clipped_to_it() {
    let mut soft = SoftRenderer::new();
    let tex = soft.register_texture(engine_frame(32, 32));
    let shot = run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame::none(), |ui| {
            ui.viewport("scene", tex, |p, r| {
                // A white band across the middle, deliberately far wider than
                // the viewport so the clip is what stops it.
                p.rect(Rect::new(r.x - 500.0, r.center().y - 4.0, r.w + 1000.0, 8.0), Color::WHITE, 0.0);
            })
            .rect
        })
    });
    let rgb = |p: [u8; 4]| [p[0], p[1], p[2]];
    assert_eq!(rgb(shot.at(0.5, 0.5)), [255, 255, 255], "the overlay is under the frame");
    // Outside the viewport, on the same line, the overlay must not have drawn.
    let y = ((shot.rect.center().y) * shot.scale) as u32;
    let outside = shot.img.pixel((shot.rect.x * shot.scale) as u32 - 6, y);
    assert_ne!(rgb(outside), [255, 255, 255], "the overlay escaped the viewport's clip");
}

/// A sub-rect of a sheet: thumbnails, sprite pages, an atlas of previews.
#[test]
fn a_sub_rect_of_a_sheet_can_be_shown() {
    let mut soft = SoftRenderer::new();
    let tex = soft.register_texture(engine_frame(64, 64));
    let shot = run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame::none(), |ui| {
            let id = ui.make_id(("sheet", "scene"));
            let r = ui.interact(id);
            ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, r| {
                // Bottom-right quadrant only: all yellow.
                p.image_uv(r, tex, [0.5, 0.5, 1.0, 1.0], 0.0);
            });
            r.rect
        })
    });
    let rgb = |p: [u8; 4]| [p[0], p[1], p[2]];
    for (fx, fy) in [(0.25, 0.25), (0.75, 0.25), (0.25, 0.75), (0.75, 0.75)] {
        assert_eq!(rgb(shot.at(fx, fy)), [255, 255, 0], "the uv sub-rect did not select one quadrant");
    }
}

/// What the contract gives up, stated as a test so nobody rediscovers it in
/// an engine: the frame's *own* per-pixel alpha is ignored — it composites as
/// opaque RGB — but a uniform tint still applies, which is how a viewport is
/// dimmed while a modal is up, or cross-faded between two renderers.
#[test]
fn a_frame_has_no_per_pixel_alpha_but_can_be_tinted() {
    let mut soft = SoftRenderer::new();
    // Fully transparent in its own alpha channel, bright red in RGB.
    let mut ghosted = engine_frame(32, 32);
    for px in ghosted.data.chunks_mut(4) {
        px.copy_from_slice(&[255, 0, 0, 0]);
    }
    let tex = soft.register_texture(ghosted);

    let opaque = run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame::none(), |ui| {
            let id = ui.make_id(("sheet", "full"));
            let r = ui.interact(id);
            ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, r| {
                p.image(r, tex, 0.0);
            });
            r.rect
        })
    });
    assert_eq!(opaque.at(0.5, 0.5)[0], 255, "a zero-alpha frame vanished instead of compositing opaquely");

    let dimmed = run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame::none(), |ui| {
            let id = ui.make_id(("sheet", "dim"));
            let r = ui.interact(id);
            ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, true, move |p, r| {
                p.image_tinted(r, tex, [0.0, 0.0, 1.0, 1.0], 0.0, Color::WHITE.with_alpha(0.5));
            });
            r.rect
        })
    });
    let red = dimmed.at(0.5, 0.5)[0];
    assert!((100..=160).contains(&red), "a half-alpha tint did not fade the frame: red {red}");
}

/// A GL render target has its origin at the bottom left, so a host whose API
/// disagrees with libgui's top-left convention needs a way to flip. There is
/// no flag for it; swapping the v coordinates is the way, and this is the test
/// that makes saying so honest.
#[test]
fn swapping_the_v_coordinates_turns_an_image_over() {
    /// Red on the top half, blue on the bottom.
    fn halves() -> Texture {
        let (w, h) = (8u32, 8u32);
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let _ = x;
                let px: [u8; 4] = if y < h / 2 { [255, 0, 0, 255] } else { [0, 0, 255, 255] };
                data.extend_from_slice(&px);
            }
        }
        Texture { width: w, height: h, data }
    }

    let draw = |flip: bool| {
        let mut soft = SoftRenderer::new();
        let tex = soft.register_texture(halves());
        run(&mut soft, 1.0, move |ui| {
            let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
            ui.container(layout, Frame::none(), |ui| {
                let id = Id::new("flipped");
                let uv = if flip { [0.0, 1.0, 1.0, 0.0] } else { [0.0, 0.0, 1.0, 1.0] };
                ui.add_leaf(id, Layout::leaf(Size::Grow(1.0), Size::Grow(1.0)), Vec2::ZERO, false, move |p, r| {
                    p.image_uv(r, tex, uv, 0.0);
                });
                ui.rect_of(id).unwrap_or_default()
            })
        })
    };

    let upright = draw(false);
    let flipped = draw(true);
    // A quarter down and three quarters down, well inside each half.
    let redish = |px: [u8; 4]| px[0] > px[2];
    assert!(redish(upright.at(0.5, 0.25)), "the top is not the top: {:?}", upright.at(0.5, 0.25));
    assert!(!redish(upright.at(0.5, 0.75)), "the bottom is not the bottom");
    assert!(!redish(flipped.at(0.5, 0.25)), "swapping v did not turn it over: {:?}", flipped.at(0.5, 0.25));
    assert!(redish(flipped.at(0.5, 0.75)), "swapping v did not turn it over");
}

// ---------------------------------------------------------------------------
// Image alpha
// ---------------------------------------------------------------------------

/// Draw `tex` scaled up into a 100 px square on a plain background, so every
/// edge is filtered, and hand back the shot.
fn icon_shot(texture: Texture, alpha: ImageAlpha, background: Color) -> Shot {
    let mut soft = SoftRenderer::new();
    let tex = soft.register_texture(texture);
    run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame { fill: background, ..Frame::none() }, |ui| {
            let id = Id::new("icon");
            ui.add_leaf(id, Layout::leaf(Size::Fixed(100.0), Size::Fixed(100.0)), Vec2::ZERO, false, move |p, r| {
                p.image_with_alpha(r, tex, [0.0, 0.0, 1.0, 1.0], 0.0, Color::WHITE, alpha);
            });
            ui.rect_of(id).unwrap_or_default()
        })
    })
}

/// An 8x8 icon: an opaque square in the middle of a transparent field. The
/// transparent texels carry `outside` as their colour, which is what decides
/// whether a wrong filter shows: transparent *black* is what an image editor
/// usually writes.
fn square_icon(inside: [u8; 3], outside: [u8; 3], premultiplied: bool) -> Texture {
    let (w, h) = (8u32, 8u32);
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let core = (2..6).contains(&x) && (2..6).contains(&y);
            let (rgb, a) = if core { (inside, 255u8) } else { (outside, 0u8) };
            let k = if premultiplied { a as u32 } else { 255 };
            data.extend_from_slice(&[
                (rgb[0] as u32 * k / 255) as u8,
                (rgb[1] as u32 * k / 255) as u8,
                (rgb[2] as u32 * k / 255) as u8,
                a,
            ]);
        }
    }
    Texture { width: w, height: h, data }
}

/// The default reads no alpha at all: a 3D view's alpha usually means
/// nothing, and honouring it would punch holes in the scene.
#[test]
fn an_opaque_image_ignores_its_alpha() {
    let shot = icon_shot(square_icon([255, 0, 0], [0, 0, 255], false), ImageAlpha::Opaque, Color::WHITE);
    // The corner is a transparent texel, drawn anyway: blue, not the white
    // background behind it.
    let corner = shot.at(0.05, 0.05);
    assert!(corner[2] > 200 && corner[0] < 50, "opaque mode let the background through: {corner:?}");
}

/// Straight alpha lets the background through where the icon is transparent,
/// and covers it where the icon is solid.
#[test]
fn a_straight_alpha_image_is_see_through_where_it_is_transparent() {
    let shot = icon_shot(square_icon([255, 0, 0], [0, 0, 0], false), ImageAlpha::Straight, Color::WHITE);
    let corner = shot.at(0.05, 0.05);
    let middle = shot.at(0.5, 0.5);
    assert!(corner.iter().take(3).all(|&c| c > 250), "the transparent corner is not the background: {corner:?}");
    assert!(middle[0] > 250 && middle[1] < 5 && middle[2] < 5, "the solid middle is not the icon: {middle:?}");
}

/// The test that matters. A white icon on a white background, with the
/// transparent texels around it black, as image editors usually write them:
/// filtered correctly, every pixel is white. Premultiply after filtering
/// instead and the black bleeds into every edge — the dark halo a
/// hand-written icon renderer gets on its first try.
#[test]
fn a_straight_alpha_icon_has_no_dark_halo() {
    let shot = icon_shot(square_icon([255, 255, 255], [0, 0, 0], false), ImageAlpha::Straight, Color::WHITE);
    let mut darkest = 255u8;
    for i in 0..=40 {
        for j in 0..=40 {
            let px = shot.at(i as f32 / 40.0, j as f32 / 40.0);
            darkest = darkest.min(px[0]).min(px[1]).min(px[2]);
        }
    }
    assert!(darkest >= 254, "a white icon on white has a dark edge: darkest channel {darkest}");
}

/// A texture premultiplied on upload, drawn as premultiplied, is the same
/// picture as the straight original drawn as straight. That is the path a
/// renderer that filters in hardware has to take, so it has to agree.
#[test]
fn premultiplied_on_upload_draws_what_straight_draws() {
    let straight = icon_shot(square_icon([40, 160, 220], [0, 0, 0], false), ImageAlpha::Straight, Color::hex(0x202020));
    let premul = icon_shot(square_icon([40, 160, 220], [0, 0, 0], true), ImageAlpha::Premultiplied, Color::hex(0x202020));
    let worst = straight.img.data.iter().zip(&premul.img.data).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
    assert!(worst <= 1, "premultiplying on upload changed the picture by {worst} levels");
}

/// The tint multiplies an icon's alpha as well as its colour, so one white
/// icon can be faded for a disabled state.
#[test]
fn a_tint_fades_an_icon_without_brightening_it() {
    let mut soft = SoftRenderer::new();
    let tex = soft.register_texture(square_icon([255, 255, 255], [0, 0, 0], false));
    let shot = run(&mut soft, 1.0, move |ui| {
        let layout = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(20.0));
        ui.container(layout, Frame { fill: Color::BLACK, ..Frame::none() }, |ui| {
            let id = Id::new("icon");
            ui.add_leaf(id, Layout::leaf(Size::Fixed(100.0), Size::Fixed(100.0)), Vec2::ZERO, false, move |p, r| {
                p.image_with_alpha(r, tex, [0.0, 0.0, 1.0, 1.0], 0.0, Color::WHITE.with_alpha(0.5), ImageAlpha::Straight);
            });
            ui.rect_of(id).unwrap_or_default()
        })
    });
    let middle = shot.at(0.5, 0.5);
    assert!((120..=135).contains(&middle[0]), "half-faded white on black is not mid-grey: {middle:?}");
}
