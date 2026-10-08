// One shader for the whole UI. Each instance is a quad whose kind is
// Instance.params.w (see libgui::render_contract::PrimitiveKind):
//   KIND_SHAPE: rounded-rect SDF (fills, borders, soft shadows)
//   KIND_GLYPH: glyph (coverage from the R8 atlas)
//   KIND_IMAGE: image with rounded-corner mask (e.g. engine viewport)
//   KIND_LINE:  line segment with round caps (wires, curves, waveforms),
//               optionally dashed
//   KIND_TRIANGLE: a filled triangle; a polygon is several. Its quad is put
//               on the triangle itself (padded for anti-aliasing), not on rect
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
    // Flat: a triangle's third corner rides here, and two triangles sharing
    // an edge must see bit-identical corners for the seam rule to hold.
    @location(3) @interpolate(flat) border_color: vec4<f32>,
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

    // A triangle's quad is the triangle, its corners pushed out for the
    // anti-aliased fringe (render_contract::triangle_corners): corners 0, 1, 2
    // and 2 again, so the quad's second triangle is empty.
    if (kind == KIND_TRIANGLE) {
        let q = triangle_corners(i.uv.xy, i.uv.zw, i.border_color.xy);
        world = q[min(vi_corner(vi), 2u)];
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

// Which quad corner vertex `vi` is: 0 top-left, 1 top-right, 2 bottom-left,
// 3 bottom-right, for the two triangles (0,1,2) and (2,1,3).
fn vi_corner(vi: u32) -> u32 {
    var m = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    return m[vi];
}

// The unit normal pointing out of a triangle from edge a->b (the contract's
// winding: cross(b - a, c - a) > 0).
fn tri_outward(a: vec2<f32>, b: vec2<f32>) -> vec2<f32> {
    let d = b - a;
    let l = sqrt(d.x * d.x + d.y * d.y);
    if (l <= 0.0) {
        return vec2(0.0, 0.0);
    }
    return vec2(d.y / l, -d.x / l);
}

// render_contract::triangle_corners: each corner pushed out so every edge
// moves out by TRIANGLE_PAD, clamped at needle-sharp corners.
fn triangle_corners(a: vec2<f32>, b: vec2<f32>, c: vec2<f32>) -> array<vec2<f32>, 3> {
    var v = array<vec2<f32>, 3>(a, b, c);
    var out: array<vec2<f32>, 3>;
    for (var i = 0u; i < 3u; i++) {
        let p = v[i];
        let prev = v[(i + 2u) % 3u];
        let next = v[(i + 1u) % 3u];
        let n1 = tri_outward(prev, p);
        let n2 = tri_outward(p, next);
        let m = n1 + n2;
        let d = 1.0 + n1.x * n2.x + n1.y * n2.y;
        var k = 0.0;
        if (d > 1e-6) {
            k = min(TRIANGLE_PAD / d, TRIANGLE_PAD * TRIANGLE_MITER_LIMIT * 0.5);
        }
        out[i] = vec2(p.x + m.x * k, p.y + m.y * k);
    }
    return out;
}

// render_contract::triangle_edge: one edge's coverage. The endpoints are put
// in a fixed order so two triangles sharing an edge compute the same number,
// exactly, with opposite signs; a pixel centre on the edge goes to the one
// running along it downward (or leftward).
fn triangle_edge(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, outer: bool, aa: f32) -> f32 {
    var lo = a;
    var hi = b;
    var sgn = 1.0;
    if (b.x < a.x || (b.x == a.x && b.y < a.y)) {
        lo = b;
        hi = a;
        sgn = -1.0;
    }
    let d = hi - lo;
    let e = sgn * (d.x * (p.y - lo.y) - d.y * (p.x - lo.x));
    if (outer) {
        let len = max(sqrt(d.x * d.x + d.y * d.y), 1e-12);
        return clamp(0.5 + e / (len * aa), 0.0, 1.0);
    }
    if (e > 0.0) {
        return 1.0;
    }
    if (e < 0.0) {
        return 0.0;
    }
    let dir = b - a;
    if (dir.y > 0.0 || (dir.y == 0.0 && dir.x < 0.0)) {
        return 1.0;
    }
    return 0.0;
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
    if (kind == KIND_TRIANGLE) {
        // The pixel centre itself, not the interpolated `world`: two
        // triangles sharing an edge interpolate across different corners,
        // and land a last bit apart — enough for both to claim a pixel on the
        // edge, or neither. The hardware's pixel centre is the same number in
        // both, which is what makes the shared-edge test exact.
        let p = v.pos.xy / g.scale;
        let edges = u32(round(v.border_color.z));
        let a = v.seg.xy;
        let b = v.seg.zw;
        let c = v.border_color.xy;
        var cov = triangle_edge(p, a, b, (edges & 1u) != 0u, aa);
        cov = min(cov, triangle_edge(p, b, c, (edges & 2u) != 0u, aa));
        cov = min(cov, triangle_edge(p, c, a, (edges & 4u) != 0u, aa));
        return premul(v.color) * cov;
    }
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
