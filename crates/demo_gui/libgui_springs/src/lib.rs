//! A small demo of springs: three things that move, and the two numbers that
//! decide how.
//!
//! - **Interrupt it.** Three switches flipped together — an ease, a
//!   critically damped spring, a bouncy one. Flip them twice quickly: the ease
//!   reverses on the spot, the springs carry on for a moment and turn.
//! - **Throw it.** A puck you drag and let go. It keeps the velocity you threw
//!   it with and settles in whichever slot that velocity carries it to.
//! - **Tune it.** Response and damping, with the step response drawn from the
//!   same solution the widgets follow.
//!
//! Everything here is app code over four calls: `animate_spring_with`,
//! `set_spring`, `pointer_velocity` and `Spring::value_at`.

use libgui::*;

/// Where the puck can come to rest, as fractions of its track.
pub const SLOTS: [f32; 3] = [0.1, 0.5, 0.9];

const TRACK_W: f32 = 520.0;
const TRACK_H: f32 = 96.0;
const PUCK_R: f32 = 26.0;

pub struct Springs {
    /// The three switches share this.
    pub on: bool,
    pub response: f32,
    pub damping: f32,
    pub reduced_motion: bool,
    /// The slot the puck is going to.
    pub slot: usize,
    /// While dragging: the puck's centre in track space, and where on it the
    /// pointer took hold.
    drag: Option<(f32, f32)>,
    /// Where the puck was drawn last frame, in track space.
    pub shown_x: f32,
}

impl Default for Springs {
    fn default() -> Self {
        Self { on: false, response: 0.4, damping: 0.55, reduced_motion: false, slot: 0, drag: None, shown_x: SLOTS[0] * TRACK_W }
    }
}

impl Springs {
    pub fn spring(&self) -> Spring {
        Spring::new(self.response, self.damping)
    }

    /// One frame of the whole demo.
    pub fn ui(&mut self, ui: &mut Ui) {
        ui.theme.metrics.reduced_motion = self.reduced_motion;
        let t = ui.theme.clone();
        let page = Layout::column().width(Size::Grow(1.0)).height(Size::Grow(1.0)).padding(Insets::all(28.0)).gap(18.0);
        ui.container(page, Frame { fill: t.palette.bg_app, ..Frame::none() }, |ui| {
            ui.heading("Springs");
            ui.label_muted("Motion that carries velocity, so it bends instead of reversing.");

            self.interrupt(ui);
            self.throw(ui);
            self.tune(ui);
        });
    }

    fn section(ui: &mut Ui, title: &str, hint: &str) {
        ui.label(title);
        ui.label_muted(hint);
    }

    /// Three switches, one flag: an ease against two springs.
    fn interrupt(&mut self, ui: &mut Ui) {
        Self::section(ui, "Interrupt it", "Flip twice quickly. The ease reverses on the spot; the springs turn.");
        let spring = self.spring();
        ui.row(|ui| {
            if ui.button("Flip all three").clicked {
                self.on = !self.on;
            }
            let target = if self.on { 1.0 } else { 0.0 };
            let ease = ui.animate_with_speed(Id::new("ease"), 0, target, 9.0);
            let critical = ui.animate_spring_with(Id::new("critical"), 0, target, Spring::new(spring.response, 1.0));
            let bouncy = ui.animate_spring_with(Id::new("bouncy"), 0, target, spring);
            for (key, label, v) in [("ease", "Ease", ease), ("critical", "Spring", critical), ("bouncy", "Your spring", bouncy)] {
                switch(ui, key, label, v);
            }
        });
    }

    /// A puck you can throw between three slots.
    fn throw(&mut self, ui: &mut Ui) {
        Self::section(ui, "Throw it", "Drag the puck and let go. It keeps your velocity and settles where that carries it.");
        let id = Id::new("puck");
        let resp = ui.interact_drag(id);
        let track = resp.rect;
        let home = |slot: usize| SLOTS[slot] * TRACK_W;

        // Where the spring has the puck this frame. While a drag holds it,
        // this is overwritten below with where the pointer has it.
        let now = ui.animate_spring_with(id, 0, home(self.slot), self.spring());
        let local_x = resp.mouse_pos.x - track.x;

        if resp.pressed && (local_x - now).abs() <= PUCK_R * 1.4 {
            self.drag = Some((now, local_x - now));
        }
        let mut x = now;
        if let Some((_, grab)) = self.drag {
            // Where the pointer has it, on the frame it is held and on the
            // frame it is let go.
            x = (local_x - grab).clamp(PUCK_R, TRACK_W - PUCK_R);
            if resp.released {
                // Let go, this frame: hand the throw to the spring, and aim
                // for the slot the throw would carry it to — a quarter of a
                // second of coasting decides which. (`released` before
                // `active`: active is still true on this frame.)
                let v = ui.pointer_velocity().x;
                ui.set_spring(id, 0, x, v);
                let lands = x + v * 0.25;
                self.slot = (0..SLOTS.len())
                    .min_by(|&a, &b| (home(a) - lands).abs().total_cmp(&(home(b) - lands).abs()))
                    .unwrap_or(0);
                self.drag = None;
            } else if resp.active {
                // Held: the spring is told where the pointer has it, at rest,
                // so letting go starts from here.
                ui.set_spring(id, 0, x, 0.0);
                self.drag = Some((x, grab));
            } else {
                // Lost the press without a release (focus went elsewhere):
                // let the spring take it home.
                self.drag = None;
            }
        }

        self.shown_x = x;
        let t = ui.theme.clone();
        let dragging = self.drag.is_some();
        let slot = self.slot;
        ui.add_leaf(id, Layout::leaf(Size::Fixed(TRACK_W), Size::Fixed(TRACK_H)), Vec2::ZERO, true, move |p, r| {
            p.rect(r, t.palette.bg_inset, t.metrics.radius_large);
            let cy = r.y + r.h * 0.5;
            for (i, s) in SLOTS.iter().enumerate() {
                let c = Vec2::new(r.x + s * r.w, cy);
                let ring = if i == slot { t.palette.accent.with_alpha(0.55) } else { t.palette.border };
                p.rect_bordered(Rect::new(c.x - PUCK_R, c.y - PUCK_R, PUCK_R * 2.0, PUCK_R * 2.0), Color::TRANSPARENT, PUCK_R, 2.0, ring);
            }
            let c = Vec2::new(r.x + x, cy);
            let puck = Rect::new(c.x - PUCK_R, c.y - PUCK_R, PUCK_R * 2.0, PUCK_R * 2.0);
            p.shadow(puck.translate(0.0, 4.0), PUCK_R, if dragging { 18.0 } else { 10.0 }, t.palette.shadow);
            p.rect(puck, t.palette.accent, PUCK_R);
        });
    }

    /// The two numbers, the reduced-motion switch, and the curve they make.
    fn tune(&mut self, ui: &mut Ui) {
        Self::section(ui, "Tune it", "Response is roughly how long the motion takes; damping 1 is no overshoot.");
        let row = Layout::row().width(Size::Grow(1.0)).height(Size::Fit).gap(24.0);
        ui.container(row, Frame::none(), |ui| {
            let controls = Layout::column().width(Size::Fixed(300.0)).height(Size::Fit).gap(8.0);
            ui.container(controls, Frame::none(), |ui| {
                ui.slider("Response (s)", &mut self.response, 0.1, 1.0);
                ui.slider("Damping", &mut self.damping, 0.2, 2.0);
                ui.checkbox("Reduced motion", &mut self.reduced_motion);
            });
            let spring = self.spring();
            let t = ui.theme.clone();
            let id = ui.make_id("curve");
            ui.add_leaf(id, Layout::leaf(Size::Fixed(260.0), Size::Fixed(120.0)), Vec2::ZERO, false, move |p, r| {
                p.rect(r, t.palette.bg_inset, t.metrics.radius);
                // 0 at the bottom, 1 two-thirds up, so an overshoot has room.
                let y_of = |v: f32| r.bottom() - 8.0 - v * (r.h - 16.0) / 1.5;
                p.line(Vec2::new(r.x + 6.0, y_of(1.0)), Vec2::new(r.right() - 6.0, y_of(1.0)), 1.0, t.palette.border);
                let span = (spring.response * 2.0).max(0.2);
                let pts: Vec<Vec2> = (0..=64)
                    .map(|i| {
                        let f = i as f32 / 64.0;
                        Vec2::new(r.x + 6.0 + f * (r.w - 12.0), y_of(spring.value_at(f * span)))
                    })
                    .collect();
                p.polyline(&pts, 2.0, t.palette.accent);
            });
        });
    }
}

/// A switch whose knob sits at `v` of the way across: 0 off, 1 on, and
/// anything — including past either end — in between.
fn switch(ui: &mut Ui, key: &str, label: &str, v: f32) {
    let t = ui.theme.clone();
    let col = Layout::column().width(Size::Fit).height(Size::Fit).gap(6.0);
    let outer = ui.make_id(("switch", key));
    ui.container_id(outer, col, Frame::none(), |ui| {
        let id = ui.make_id(("track", key));
        ui.add_leaf(id, Layout::leaf(Size::Fixed(96.0), Size::Fixed(36.0)), Vec2::ZERO, false, move |p, r| {
            let on = t.palette.accent.with_alpha(0.25 + 0.75 * v.clamp(0.0, 1.0));
            p.rect(r, t.palette.bg_inset.lerp(on, v.clamp(0.0, 1.0)), r.h * 0.5);
            let d = r.h - 8.0;
            let x = r.x + 4.0 + v * (r.w - 8.0 - d);
            p.rect(Rect::new(x, r.y + 4.0, d, d), Color::WHITE, d * 0.5);
        });
        ui.label_muted(label);
    });
}
