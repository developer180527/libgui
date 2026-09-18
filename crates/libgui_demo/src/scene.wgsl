// Stand-in for your engine's renderer: an orbit camera, a spinning lit cube and
// an infinite-looking grid, drawn into an offscreen target that the UI shows.

struct U {
    cam: vec4<f32>,   // yaw, pitch, distance, cube scale
    misc: vec4<f32>,  // aspect, spin, 0, 0
};
@group(0) @binding(0) var<uniform> u: U;

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) normal: vec3<f32>,
    @location(1) world: vec3<f32>,
    @location(2) @interpolate(flat) kind: f32,
};

fn rot_y(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a); let s = sin(a);
    return vec3(c * p.x + s * p.z, p.y, -s * p.x + c * p.z);
}
fn rot_x(p: vec3<f32>, a: f32) -> vec3<f32> {
    let c = cos(a); let s = sin(a);
    return vec3(p.x, c * p.y - s * p.z, s * p.y + c * p.z);
}

@vertex
fn vs_main(@location(0) p: vec3<f32>, @location(1) n: vec3<f32>, @location(2) kind: f32) -> VOut {
    var wp = p;
    var wn = n;
    if (kind < 0.5) {
        wp = rot_y(p * u.cam.w, u.misc.y) + vec3(0.0, u.cam.w, 0.0);
        wn = rot_y(n, u.misc.y);
    }
    let v = rot_x(rot_y(wp, u.cam.x), u.cam.y) - vec3(0.0, 0.8, u.cam.z);
    let near = 0.05; let far = 200.0;
    let f = 1.0 / tan(0.5 * 0.9);
    let a = far / (near - far);
    let b = near * far / (near - far);
    var o: VOut;
    o.pos = vec4(v.x * f / u.misc.x, v.y * f, a * v.z + b, -v.z);
    o.normal = wn;
    o.world = wp;
    o.kind = kind;
    return o;
}

@fragment
fn fs_main(v: VOut) -> @location(0) vec4<f32> {
    let bg = vec3(0.075, 0.075, 0.082);
    // Derivatives must be taken in uniform control flow.
    let fw = fwidth(v.world.xz);
    if (v.kind > 0.5) {
        let gp = v.world.xz;
        let g = abs(fract(gp - 0.5) - 0.5) / max(fw, vec2(1e-4));
        let line = 1.0 - min(min(g.x, g.y), 1.0);
        let g5 = abs(fract(gp / 5.0 - 0.5) - 0.5) / max(fw / 5.0, vec2(1e-4));
        let major = 1.0 - min(min(g5.x, g5.y), 1.0);
        let fade = exp(-length(gp) * 0.09);
        var c = mix(bg, vec3(0.22, 0.22, 0.24), line * fade * 0.7);
        c = mix(c, vec3(0.33, 0.33, 0.36), major * fade);
        let ax = 1.0 - min(abs(gp.y) / max(fw.y, 1e-4), 1.0);
        let az = 1.0 - min(abs(gp.x) / max(fw.x, 1e-4), 1.0);
        c = mix(c, vec3(0.85, 0.30, 0.30), ax * fade);
        c = mix(c, vec3(0.30, 0.45, 0.90), az * fade);
        return vec4(c, 1.0);
    }
    let n = normalize(v.normal);
    let l = normalize(vec3(0.5, 0.9, 0.35));
    let base = mix(vec3(0.36, 0.55, 0.94), vec3(0.95, 0.55, 0.30), clamp(n.y * 0.5 + 0.5 - abs(n.x) * 0.4, 0.0, 1.0));
    let diff = max(dot(n, l), 0.0);
    let c = base * (0.25 + 0.8 * diff);
    return vec4(c, 1.0);
}
