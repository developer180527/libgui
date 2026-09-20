//! The edit: what the app owns. libgui holds none of it.

use libgui::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Video,
    Audio,
    Title,
}

/// One clip on one track. Times are seconds on the sequence timeline.
#[derive(Clone, Debug)]
pub struct Clip {
    pub name: String,
    pub track: usize,
    pub start: f32,
    pub len: f32,
    pub media: Media,
    /// Has effects applied: the `fx` badge in the corner.
    pub fx: bool,
    /// Tint of the thumbnail strip, so clips are told apart at a glance.
    pub tint: Color,
}

impl Clip {
    pub fn end(&self) -> f32 {
        self.start + self.len
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrackKind {
    Video,
    Audio,
}

#[derive(Clone, Debug)]
pub struct Track {
    pub name: &'static str,
    pub kind: TrackKind,
    pub height: f32,
    pub locked: bool,
    /// Video: the eye. Audio: mute.
    pub visible: bool,
    pub muted: bool,
    pub solo: bool,
    /// Targeted for insert/overwrite — the highlighted V1 / A1 button.
    pub targeted: bool,
}

/// An item in the project bin.
#[derive(Clone, Debug)]
pub struct Asset {
    pub name: &'static str,
    pub duration: &'static str,
    pub media: Media,
    /// Two colours the thumbnail is painted from, standing in for a frame.
    pub tint: (Color, Color),
}

/// The timeline's fixed furniture, in logical px: the header column's width,
/// the ruler's height, and the scrollbars'.
pub const HEADER_W: f32 = 148.0;
pub const RULER_H: f32 = 26.0;
pub const BAR: f32 = 11.0;

/// What is being dragged on the timeline.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Drag {
    /// Moving a clip: which one, and where in it the pointer grabbed.
    Clip { index: usize, grab: f32 },
    Playhead,
    ScrollX,
    ScrollY,
}

pub struct Sequence {
    pub name: &'static str,
    pub tracks: Vec<Track>,
    pub clips: Vec<Clip>,
    /// Seconds.
    pub duration: f32,
}

pub struct Editor {
    pub project: &'static str,
    pub seq: Sequence,
    pub assets: Vec<Asset>,
    pub selected_asset: usize,
    pub selected_clip: Option<usize>,
    /// Seconds.
    pub playhead: f32,
    pub playing: bool,
    /// Timeline zoom, px per second.
    pub pps: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
    pub drag: Option<Drag>,
    /// Where the timeline's surface landed last frame. The app needs it to
    /// turn a pointer position into a time, and it is what a test drives.
    pub timeline_rect: Rect,
    pub tool: usize,
    /// Bin thumbnail size, 0..1.
    pub thumb: f32,
    pub top_tab: usize,
    pub snap: bool,
    pub linked: bool,
    pub markers: bool,
    /// Effect Controls: the property values, and which are keyframed.
    pub position: (f32, f32),
    pub scale: f32,
    pub scale_width: f32,
    pub uniform_scale: bool,
    pub rotation: f32,
    pub anchor: (f32, f32),
    pub anti_flicker: f32,
    pub crop: [f32; 4],
    pub opacity_open: bool,
    pub motion_open: bool,
    pub twirl_open: bool,
    /// Program monitor.
    pub fit: usize,
    pub quality: usize,
    /// Audio meter levels, 0..1 per channel.
    pub meters: [f32; 2],
    pub time: f32,
}

pub fn sequence() -> Sequence {
    let tracks = vec![
        Track { name: "V2", kind: TrackKind::Video, height: 46.0, locked: false, visible: true, muted: false, solo: false, targeted: false },
        Track { name: "V1", kind: TrackKind::Video, height: 62.0, locked: false, visible: true, muted: false, solo: false, targeted: true },
        Track { name: "A1", kind: TrackKind::Audio, height: 58.0, locked: false, visible: true, muted: false, solo: false, targeted: true },
        Track { name: "A2", kind: TrackKind::Audio, height: 44.0, locked: false, visible: true, muted: false, solo: false, targeted: false },
        Track { name: "A3", kind: TrackKind::Audio, height: 44.0, locked: false, visible: true, muted: false, solo: false, targeted: false },
    ];
    let v = Color::hex(0x3f7f5f);
    let clips = vec![
        Clip { name: "Tikal.mp4 [V]".into(), track: 1, start: 0.0, len: 6.4, media: Media::Video, fx: true, tint: v },
        Clip { name: "Atitlan.mp4 [V]".into(), track: 1, start: 6.4, len: 1.6, media: Media::Video, fx: true, tint: Color::hex(0x3b5f8a) },
        Clip { name: "AntiguaArchTL.mp4 [V]".into(), track: 1, start: 8.0, len: 7.2, media: Media::Video, fx: true, tint: Color::hex(0x8a6a4a) },
        Clip { name: "DockAtitlanTL.mp4 [V]".into(), track: 0, start: 3.6, len: 4.2, media: Media::Video, fx: true, tint: Color::hex(0x3b5f8a) },
        Clip { name: "HIKING".into(), track: 0, start: 9.2, len: 4.0, media: Media::Title, fx: true, tint: Color::hex(0x7a3fa8) },
        Clip { name: "Tikal.mp4 [A]".into(), track: 2, start: 0.0, len: 6.4, media: Media::Audio, fx: true, tint: Color::hex(0x2a6079) },
        Clip { name: "Atitlan.mp4 [A]".into(), track: 2, start: 6.4, len: 1.6, media: Media::Audio, fx: false, tint: Color::hex(0x2a6079) },
        Clip { name: "AntiguaArchTL.mp4 [A]".into(), track: 2, start: 8.0, len: 7.2, media: Media::Audio, fx: true, tint: Color::hex(0x2a6079) },
        Clip { name: "Music_Bed.wav".into(), track: 3, start: 0.6, len: 13.0, media: Media::Audio, fx: false, tint: Color::hex(0x2a6079) },
    ];
    Sequence { name: "Sequence 01", tracks, clips, duration: 18.0 }
}

fn assets() -> Vec<Asset> {
    let a = |name, duration, media, c0, c1| Asset { name, duration, media, tint: (Color::hex(c0), Color::hex(c1)) };
    vec![
        a("DockAtitlanTL.mp4", "1:00", Media::Video, 0x2f6f9e, 0x8fc6a8),
        a("AdobeStock_1166400934.mp4", "4:05", Media::Video, 0x9a8f7a, 0xd8cfc0),
        a("Tikal.mp4", "0:22", Media::Video, 0x4a7f4a, 0xa8d08a),
        a("AntiguaArchTL.mp4", "2:14", Media::Video, 0x8a6a4a, 0xe0c9a0),
        a("Atitlan.mp4", "0:48", Media::Video, 0x2f5f8a, 0x9ec8e8),
        a("HIKING (title)", "0:04", Media::Title, 0x7a3fa8, 0xc9a0e0),
        a("Music_Bed.wav", "3:31", Media::Audio, 0x2a6079, 0x7fc6e8),
        a("Ambience_Jungle.wav", "5:12", Media::Audio, 0x2a6079, 0x7fc6e8),
        a("Interview_01.mp4", "8:40", Media::Video, 0x6a4a4a, 0xd0a0a0),
        a("B-roll_Market.mp4", "1:37", Media::Video, 0x7a6a3a, 0xd8c890),
    ]
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            project: "Hiking",
            seq: sequence(),
            assets: assets(),
            selected_asset: 1,
            selected_clip: Some(0),
            playhead: 7.0 / 24.0,
            playing: false,
            pps: 78.0,
            scroll_x: 0.0,
            scroll_y: 0.0,
            drag: None,
            timeline_rect: Rect::default(),
            tool: 0,
            thumb: 0.45,
            top_tab: 1,
            snap: true,
            linked: true,
            markers: false,
            position: (179.0, 180.0),
            scale: 186.0,
            scale_width: 100.0,
            uniform_scale: true,
            rotation: 0.0,
            anchor: (320.0, 180.0),
            anti_flicker: 0.0,
            crop: [0.0; 4],
            opacity_open: false,
            motion_open: true,
            twirl_open: true,
            fit: 0,
            quality: 0,
            meters: [0.62, 0.55],
            time: 0.0,
        }
    }
}

/// `seconds` as `hh:mm:ss:ff` at 24 fps, the way an editor shows time.
pub fn timecode(seconds: f32) -> String {
    let fps = 24.0;
    let total = (seconds.max(0.0) * fps).round() as u32;
    let f = total % 24;
    let s = (total / 24) % 60;
    let m = (total / (24 * 60)) % 60;
    let h = total / (24 * 60 * 60);
    format!("{h:02}:{m:02}:{s:02}:{f:02}")
}

impl Editor {
    /// Longest clip end, with room to keep cutting.
    pub fn content_end(&self) -> f32 {
        self.seq.clips.iter().fold(0.0f32, |a, c| a.max(c.end())).max(1.0)
    }

    pub fn tracks_height(&self) -> f32 {
        self.seq.tracks.iter().map(|t| t.height + 1.0).sum()
    }

    /// Top of `track` within the track stack, ignoring scroll. Video tracks
    /// stack upward from the middle, audio downward, as an NLE does.
    pub fn track_top(&self, track: usize) -> f32 {
        self.seq.tracks.iter().take(track).map(|t| t.height + 1.0).sum()
    }

    /// Window position of `time` on `track`, inside the timeline's surface.
    pub fn point_at(&self, time: f32, track: usize) -> Vec2 {
        let r = self.timeline_rect;
        let x = r.x + HEADER_W + time * self.pps - self.scroll_x;
        let y = r.y + RULER_H + self.track_top(track) - self.scroll_y + self.seq.tracks[track].height * 0.5;
        Vec2::new(x, y)
    }

    /// Window position of `time` on the ruler.
    pub fn ruler_point(&self, time: f32) -> Vec2 {
        let r = self.timeline_rect;
        Vec2::new(r.x + HEADER_W + time * self.pps - self.scroll_x, r.y + RULER_H * 0.5)
    }

    pub fn clip_at(&self, track: usize, t: f32) -> Option<usize> {
        self.seq
            .clips
            .iter()
            .position(|c| c.track == track && t >= c.start && t < c.end())
    }

    /// Advance playback and the meters. `dt` seconds.
    pub fn tick(&mut self, dt: f32) {
        self.time += dt;
        if self.playing {
            self.playhead = (self.playhead + dt) % self.content_end();
        }
        let base = if self.playing { 0.75 } else { 0.0 };
        for (i, m) in self.meters.iter_mut().enumerate() {
            let wobble = ((self.time * (3.1 + i as f32 * 0.7)).sin() * 0.5 + 0.5) * 0.25;
            *m = (base + wobble * base).min(1.0);
        }
    }
}
