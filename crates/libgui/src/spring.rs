//! A damped spring, advanced exactly.
//!
//! `Ui::animate` is a first-order ease: each frame closes a fixed fraction of
//! the gap. It has no velocity, so when its target changes mid-flight — a
//! panel told to close while it is still opening — the motion reverses
//! instantly, which nothing physical does and which reads as mechanical. A
//! spring is second-order. It carries velocity, so an interrupted motion bends
//! smoothly toward its new target instead of snapping round.
//!
//! Each step applies the **closed-form solution** of the damped harmonic
//! oscillator for that interval, rather than integrating numerically. So:
//!
//! - the position at a moment depends only on the time elapsed, never on how
//!   the time was cut into frames — 30, 60 or 144 Hz trace the same curve;
//! - a long frame (a stall, a resumed app) cannot make it unstable or fling it
//!   past its target, which a numerical integrator does without substeps;
//! - it costs a handful of float operations and one `exp`.

use crate::theme::Spring;

/// Position and velocity, in the animated value's units and units/s.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct State {
    pub pos: f32,
    pub vel: f32,
}

impl Spring {
    /// Where this spring is `t` seconds after being let go at 0, at rest,
    /// toward 1: its step response.
    ///
    /// For plotting a spring while tuning it — the curve drawn is the one
    /// [`Ui::animate_spring_with`](crate::Ui::animate_spring_with) follows,
    /// because it is the same exact solution.
    pub fn value_at(&self, t: f32) -> f32 {
        let mut s = State::default();
        step(&mut s, 1.0, *self, t.max(0.0));
        s.pos
    }
}

/// Below these it is at rest: snapped onto the target, velocity zero, and no
/// longer asking for frames.
const REST_POS: f32 = 1e-3;
const REST_VEL: f32 = 1e-2;

/// Advance `s` toward `target` by `dt` seconds. Returns true when it has come
/// to rest on the target.
pub(crate) fn step(s: &mut State, target: f32, spring: Spring, dt: f32) -> bool {
    if !(s.pos.is_finite() && s.vel.is_finite()) {
        *s = State { pos: target, vel: 0.0 };
        return true;
    }
    // Degenerate parameters fall back to something that still arrives: a
    // response of zero or less is "instantly", damping below zero is not a
    // spring at all.
    let response = if spring.response.is_finite() { spring.response } else { 0.0 };
    let zeta = if spring.damping.is_finite() { spring.damping.max(0.0) } else { 1.0 };
    if response <= 1e-4 || dt <= 0.0 {
        if response <= 1e-4 {
            *s = State { pos: target, vel: 0.0 };
            return true;
        }
        return at_rest(s, target);
    }

    let w0 = std::f32::consts::TAU / response;
    let x0 = s.pos - target;
    let v0 = s.vel;
    let t = dt;

    let (x, v) = if (zeta - 1.0).abs() < 1e-4 {
        // Critically damped: (x0 + (v0 + w0 x0) t) e^(-w0 t).
        let e = (-w0 * t).exp();
        let b = v0 + w0 * x0;
        ((x0 + b * t) * e, (v0 - w0 * b * t) * e)
    } else if zeta < 1.0 {
        // Under-damped: an oscillation at wd inside a decaying envelope.
        let wd = w0 * (1.0 - zeta * zeta).sqrt();
        let e = (-zeta * w0 * t).exp();
        let (sn, cs) = (wd * t).sin_cos();
        let x = e * (x0 * cs + (v0 + zeta * w0 * x0) / wd * sn);
        let v = e * (v0 * cs - (w0 * w0 * x0 + zeta * w0 * v0) / wd * sn);
        (x, v)
    } else {
        // Over-damped: two decaying exponentials.
        let r = (zeta * zeta - 1.0).sqrt();
        let (r1, r2) = (-w0 * (zeta - r), -w0 * (zeta + r));
        let c1 = (v0 - r2 * x0) / (r1 - r2);
        let c2 = x0 - c1;
        let (e1, e2) = ((r1 * t).exp(), (r2 * t).exp());
        (c1 * e1 + c2 * e2, c1 * r1 * e1 + c2 * r2 * e2)
    };
    s.pos = target + x;
    s.vel = v;
    at_rest(s, target)
}

fn at_rest(s: &mut State, target: f32) -> bool {
    if (s.pos - target).abs() < REST_POS && s.vel.abs() < REST_VEL {
        *s = State { pos: target, vel: 0.0 };
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Run from 0 toward 1 for `seconds` in steps of `dt`.
    fn run(spring: Spring, dt: f32, seconds: f32) -> State {
        let mut s = State::default();
        let n = (seconds / dt).round() as usize;
        for _ in 0..n {
            step(&mut s, 1.0, spring, dt);
        }
        s
    }

    /// The curve is the same however time is cut into frames: the property a
    /// numerical integrator does not have, and the reason motion looks the
    /// same on a 60 Hz laptop and a 144 Hz monitor.
    #[test]
    fn the_frame_rate_does_not_change_the_motion() {
        // 0.1 s is a whole number of frames at each of these rates, so every
        // run simulates exactly the same interval.
        for spring in [Spring::SNAPPY, Spring::BOUNCY, Spring::new(0.3, 1.8)] {
            let at = |hz: f32| run(spring, 1.0 / hz, 0.1).pos;
            let (a, b, c, d) = (at(30.0), at(60.0), at(120.0), at(240.0));
            for (hz, v) in [(60, b), (120, c), (240, d)] {
                assert!((v - a).abs() < 1e-4, "{spring:?}: 30 Hz gave {a}, {hz} Hz gave {v}");
            }
        }
    }

    /// Critically damped never passes its target; under-damped does, and
    /// over-damped is slower than critical.
    #[test]
    fn damping_decides_whether_it_overshoots() {
        let peak = |spring: Spring| {
            let mut s = State::default();
            let mut most = 0.0f32;
            for _ in 0..240 {
                step(&mut s, 1.0, spring, 1.0 / 120.0);
                most = most.max(s.pos);
            }
            most
        };
        assert!(peak(Spring::SNAPPY) <= 1.0 + 1e-6, "critical overshot to {}", peak(Spring::SNAPPY));
        assert!(peak(Spring::BOUNCY) > 1.05, "under-damped did not overshoot: {}", peak(Spring::BOUNCY));
        let half = |spring: Spring| run(spring, 1.0 / 120.0, 0.1).pos;
        assert!(half(Spring::new(0.25, 2.0)) < half(Spring::SNAPPY), "over-damped was not slower than critical");
    }

    /// It arrives, stops exactly on the target, and says so.
    #[test]
    fn it_comes_to_rest_on_the_target() {
        let mut s = State::default();
        let mut rested_after = None;
        for i in 0..600 {
            if step(&mut s, 1.0, Spring::SNAPPY, 1.0 / 60.0) {
                rested_after = Some(i);
                break;
            }
        }
        let frames = rested_after.expect("never came to rest");
        assert_eq!(s, State { pos: 1.0, vel: 0.0 }, "rest is not exactly the target");
        // A 0.25 s response should be still well inside a second.
        assert!(frames < 60, "took {frames} frames to rest");
    }

    /// Retargeting mid-flight keeps the velocity: it changes only as fast as
    /// the spring's acceleration allows, so over a short enough moment it is
    /// all but unchanged. An ease has no velocity to keep — its direction
    /// flips in zero time — which is the mechanical look this replaces.
    ///
    /// (Over a whole 60 Hz frame a *stiff* spring's velocity can change a lot:
    /// 0.6 from its target, SNAPPY accelerates at hundreds of units/s². That
    /// is the physics, not a discontinuity.)
    #[test]
    fn a_new_target_keeps_the_velocity() {
        let mut s = State::default();
        for _ in 0..6 {
            step(&mut s, 1.0, Spring::SNAPPY, 1.0 / 60.0);
        }
        let moving = s.vel;
        assert!(moving > 1.0, "should be moving toward 1 by now: {moving}");
        let before = s.pos;
        // Told to go back. A millisecond later it is still moving forward at
        // almost the same speed, and has carried on past where it was.
        step(&mut s, 0.0, Spring::SNAPPY, 0.001);
        assert!((s.vel - moving).abs() < 0.15 * moving, "the velocity jumped: {moving} -> {}", s.vel);
        assert!(s.pos > before, "it reversed in a millisecond, like an ease");
    }

    /// A stall — a frame of seconds — lands it at rest, not past its target.
    #[test]
    fn a_long_frame_cannot_fling_it() {
        for spring in [Spring::SNAPPY, Spring::BOUNCY] {
            let mut s = State { pos: 0.0, vel: 50.0 };
            step(&mut s, 1.0, spring, 5.0);
            assert!((s.pos - 1.0).abs() < 1e-3, "{spring:?}: a 5 s frame left it at {}", s.pos);
        }
    }

    /// The plotted curve is the animated one: sampling it at a time agrees
    /// with stepping there frame by frame.
    #[test]
    fn value_at_is_the_curve_the_animation_follows() {
        for spring in [Spring::SNAPPY, Spring::BOUNCY] {
            let stepped = run(spring, 1.0 / 60.0, 0.2).pos;
            assert!((spring.value_at(0.2) - stepped).abs() < 1e-4, "{spring:?}");
        }
        assert_eq!(Spring::SNAPPY.value_at(0.0), 0.0);
    }

    /// Nonsense in, something sane out.
    #[test]
    fn bad_numbers_do_not_stick() {
        let mut s = State { pos: f32::NAN, vel: 1.0 };
        assert!(step(&mut s, 2.0, Spring::SNAPPY, 1.0 / 60.0));
        assert_eq!(s.pos, 2.0);
        for spring in [Spring::new(0.0, 1.0), Spring::new(-1.0, 1.0), Spring::new(f32::NAN, 1.0)] {
            let mut s = State::default();
            assert!(step(&mut s, 3.0, spring, 1.0 / 60.0), "{spring:?} did not arrive at once");
            assert_eq!(s.pos, 3.0);
        }
        let mut s = State::default();
        step(&mut s, 1.0, Spring::new(0.3, f32::NAN), 1.0 / 60.0);
        assert!(s.pos.is_finite() && s.vel.is_finite());
    }
}
