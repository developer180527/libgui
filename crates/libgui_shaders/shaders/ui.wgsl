// One shader for the whole UI. Each instance is a quad:
//   kind 0: rounded-rect SDF (fills, borders, soft shadows)
//   kind 1: glyph (coverage from the R8 atlas)
//   kind 2: image with rounded-corner mask (e.g. engine viewport)
// Output is premultiplied alpha.
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
};

@vertex
fn vs_main(@builtin(vertex_index) vi: u32, i: Inst) -> VOut {
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[vi];
    var pad = 0.0;
    if (i.params.w < 0.5) {
        pad = i.params.z + 1.0; // room for softness + AA
    }
    let half_size = i.rect.zw * 0.5;
    let center = i.rect.xy + half_size;
    let local = (c * 2.0 - 1.0) * (half_size + vec2(pad));
    let world = center + local;

    var o: VOut;
    o.pos = vec4(world.x / g.screen.x * 2.0 - 1.0, 1.0 - world.y / g.screen.y * 2.0, 0.0, 1.0);
    o.local = local;
    o.uv = mix(i.uv.xy, i.uv.zw, c);
    o.color = i.color;
    o.border_color = i.border_color;
    o.clip = i.clip;
    o.params = i.params;
    o.half_size = half_size;
    o.world = world;
    return o;
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

@fragment
fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    if (v.world.x < v.clip.x || v.world.y < v.clip.y || v.world.x > v.clip.z || v.world.y > v.clip.w) {
        discard;
    }
    let kind = v.params.w;
    let aa = 1.0 / g.scale;

    // Sample only in the branches that need it: shapes are the bulk of UI
    // fragments and never read the texture.
    if (kind > 1.5) {
        let r = min(v.params.x, min(v.half_size.x, v.half_size.y));
        let d = sd_round_rect(v.local, v.half_size, r);
        let m = clamp(0.5 - d / aa, 0.0, 1.0);
        return vec4(sample_bilinear(v.uv).rgb, 1.0) * v.color * m;
    }
    if (kind > 0.5) {
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
