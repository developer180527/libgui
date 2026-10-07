// One shader for the whole UI. Each instance is a quad whose kind is
// Instance.params.w (see libgui::render_contract::PrimitiveKind):
//   KIND_SHAPE: rounded-rect SDF (fills, borders, soft shadows)
//   KIND_GLYPH: glyph (coverage from the R8 atlas)
//   KIND_IMAGE: image with rounded-corner mask (e.g. engine viewport)
//   KIND_LINE:  line segment with round caps (wires, curves, waveforms),
//               optionally dashed
// Images and glyphs may be rotated about their centre: the vertex stage turns
// the quad, and `local` and `uv` stay those of the upright quad, so the
// fragment stage does not know.
// Output is premultiplied alpha.
//
// CONTRACT_VERSION and the KIND_* constants are generated from
// libgui::render_contract and prepended by build.rs; do not declare them here.
//
// No sampler objects: texels are fetched with textureLoad and filtered
// manually, so every RHI only binds one uniform buffer and one texture.

struct Globals {
    screen: vec2<f32>,
    scale: f32,
    _pad: f32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(1) @binding(0) var tex: texture_2d<f32>;

struct Inst {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) clip: vec4<f32>,
    @location(5) params: vec4<f32>,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
    @location(3) border_color: vec4<f32>,
    @location(4) @interpolate(flat) clip: vec4<f32>,
    @location(5) @interpolate(flat) params: vec4<f32>,
    @location(6) @interpolate(flat) half_size: vec2<f32>,
    @location(7) world: vec2<f32>,
    // KIND_LINE endpoints; flat, because the fragment needs the segment
    // itself rather than a value interpolated across the quad.
    @location(8) @interpolate(flat) seg: vec4<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, i: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[vi];
    let kind = u32(round(i.params.w));
    // Rotation, as (cos - 1, sin) in border_color.xy: zero is upright, which
    // is what every image and glyph from before rotation carries.
    let turned = (kind == KIND_IMAGE || kind == KIND_GLYPH) && (i.border_color.x != 0.0 || i.border_color.y != 0.0);
    var pad = 0.0;
    if (kind == KIND_SHAPE) {
        pad = i.params.z + 1.0; // room for softness + AA
    } else if (turned && kind == KIND_IMAGE) {
        pad = 1.0; // a turned edge crosses pixels: room to anti-alias it
    }
    let half_size = i.rect.zw * 0.5;
    let center = i.rect.xy + half_size;
    let local = (c * 2.0 - 1.0) * (half_size + vec2(pad));
    var world = center + local;
    // Where on the texture: the corner, or for a padded turned image, the
    // same mapping carried past the edge (the mask hides what is there).
    var cuv = c;
    if (turned) {
        let cs = 1.0 + i.border_color.x;
        let sn = i.border_color.y;
        world = center + vec2(local.x * cs - local.y * sn, local.x * sn + local.y * cs);
        if (kind == KIND_IMAGE) {
            cuv = local / max(half_size, vec2(1e-6)) * 0.5 + 0.5;
        }
    }

    var o: VOut;
    o.pos = vec4(world.x / g.screen.x * 2.0 - 1.0, 1.0 - world.y / g.screen.y * 2.0, 0.0, 1.0);
    o.local = local;
    o.uv = mix(i.uv.xy, i.uv.zw, cuv);
    o.color = i.color;
    o.border_color = i.border_color;
    o.clip = i.clip;
    o.params = i.params;
    o.half_size = half_size;
    o.world = world;
    o.seg = i.uv;
    return o;
}

// Distance from p to the segment ab.
fn sd_segment(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>) -> f32 {
    let pa = p - a;
    let ba = b - a;
    let h = clamp(dot(pa, ba) / max(dot(ba, ba), 1e-6), 0.0, 1.0);
    return length(pa - ba * h);
}

fn sd_round_rect(p: vec2<f32>, b: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - b + vec2(r);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Manual bilinear filter with clamp-to-edge. Exact at texel centres, so
// pixel-snapped glyphs and 1:1 viewports stay crisp.
fn sample_bilinear(uv: vec2<f32>) -> vec4<f32> {
    let dim = vec2<i32>(textureDimensions(tex, 0));
    let p = uv * vec2<f32>(dim) - 0.5;
    let f = fract(p);
    let i = vec2<i32>(floor(p));
    let hi = dim - vec2(1);
    let a = textureLoad(tex, clamp(i, vec2(0), hi), 0);
    let b = textureLoad(tex, clamp(i + vec2(1, 0), vec2(0), hi), 0);
    let c = textureLoad(tex, clamp(i + vec2(0, 1), vec2(0), hi), 0);
    let d = textureLoad(tex, clamp(i + vec2(1, 1), vec2(0), hi), 0);
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4(c.rgb * c.a, c.a);
}

// sample_bilinear for a straight-alpha texture: each texel is premultiplied
// BEFORE it is interpolated. Doing it after lets the colour of transparent
// texels -- usually black -- bleed into every edge, which is the dark halo a
// hand-rolled icon renderer gets on its first try.
fn sample_bilinear_premul(uv: vec2<f32>) -> vec4<f32> {
    let dim = vec2<i32>(textureDimensions(tex, 0));
    let p = uv * vec2<f32>(dim) - 0.5;
    let f = fract(p);
    let i = vec2<i32>(floor(p));
    let hi = dim - vec2(1);
    let a = premul(textureLoad(tex, clamp(i, vec2(0), hi), 0));
    let b = premul(textureLoad(tex, clamp(i + vec2(1, 0), vec2(0), hi), 0));
    let c = premul(textureLoad(tex, clamp(i + vec2(0, 1), vec2(0), hi), 0));
    let d = premul(textureLoad(tex, clamp(i + vec2(1, 1), vec2(0), hi), 0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

@fragment
fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    if (v.world.x < v.clip.x || v.world.y < v.clip.y || v.world.x > v.clip.z || v.world.y > v.clip.w) {
        discard;
    }
    let kind = u32(round(v.params.w));
    let aa = 1.0 / g.scale;

    // Sample only in the branches that need it: shapes are the bulk of UI
    // fragments and never read the texture.
    if (kind == KIND_LINE) {
        var d = sd_segment(v.world, v.seg.xy, v.seg.zw) - v.params.x;
        // Dashes: params.y on, params.z off, from border_color.x into the
        // pattern at seg.xy. A signed distance along the line to the nearest
        // dash's end -- negative inside one -- intersected with the capsule,
        // so dash ends are square and anti-aliased like the sides.
        let on = v.params.y;
        let off = v.params.z;
        if (on > 0.0 && off > 0.0) {
            let ba = v.seg.zw - v.seg.xy;
            let len = max(length(ba), 1e-6);
            let s = dot(v.world - v.seg.xy, ba) / len + v.border_color.x;
            let period = on + off;
            let m = s - floor(s / period) * period;
            var along: f32;
            if (m < on) {
                along = -min(m, on - m);
            } else {
                along = min(m - on, period - m);
            }
            d = max(d, along);
        }
        let m = clamp(0.5 - d / aa, 0.0, 1.0);
        return premul(v.color) * m;
    }
    if (kind == KIND_IMAGE) {
        let r = min(v.params.x, min(v.half_size.x, v.half_size.y));
        let d = sd_round_rect(v.local, v.half_size, r);
        let m = clamp(0.5 - d / aa, 0.0, 1.0);
        // Premultiplied, like every other branch: the tint's alpha has to
        // multiply the colour as well as the coverage, or a faded viewport
        // (a dimmed scene behind a modal, a cross-fade between renderers)
        // comes out at full brightness and blends brighter than white.
        //
        // params.y is the ImageAlpha: 0 ignores the texture's alpha (a 3D
        // view whose alpha means nothing), 1 reads it as premultiplied, 2 as
        // straight -- premultiplied per texel before filtering.
        let mode = u32(round(v.params.y));
        if (mode == 0u) {
            return premul(vec4(sample_bilinear(v.uv).rgb * v.color.rgb, v.color.a)) * m;
        }
        var s: vec4<f32>;
        if (mode == 2u) {
            s = sample_bilinear_premul(v.uv);
        } else {
            s = sample_bilinear(v.uv);
        }
        // Both premultiplied now: the tint multiplies colour and alpha alike.
        return vec4(s.rgb * v.color.rgb * v.color.a, s.a * v.color.a) * m;
    }
    if (kind == KIND_GLYPH) {
        return premul(v.color) * sample_bilinear(v.uv).r;
    }

    let r = min(v.params.x, min(v.half_size.x, v.half_size.y));
    let soft = v.params.z + aa;
    let d = sd_round_rect(v.local, v.half_size, r);
    let fill = 1.0 - smoothstep(-soft * 0.5, soft * 0.5, d);
    var col = premul(v.color);
    let bw = v.params.y;
    if (bw > 0.0) {
        let inner = sd_round_rect(v.local, v.half_size - vec2(bw), max(r - bw, 0.0));
        let b = smoothstep(-aa * 0.5, aa * 0.5, inner);
        col = mix(col, premul(v.border_color), b);
    }
    return col * fill;
}
