# Licensing

libgui is **`MIT OR Apache-2.0`**, at your option — the Rust ecosystem's usual
dual licence, the same as `rustc`, `serde`, `wgpu` and `winit`. The texts are in
[`LICENSE-MIT`](LICENSE-MIT) and [`LICENSE-APACHE`](LICENSE-APACHE), and
`[workspace.package]` carries `license = "MIT OR Apache-2.0"`, which every crate
in the workspace inherits.

The dual licence was chosen over MIT alone for one reason: Apache-2.0 grants
patent rights explicitly, and some legal teams ask for that before adopting a
dependency. For a library aimed at professional tools, whose adopters are often
companies, that removes an objection and costs a second file.

This file records the obligations that come with it — the ones that are easy to
miss because nothing fails when you miss them.

## The bundled font

`assets/Inter.ttf` is **not** covered by the code licence. Inter is under the SIL
Open Font License 1.1, whose terms ship alongside it in `assets/Inter-OFL.txt`.

The OFL does not affect the licence of software that merely bundles the font, so
there is no conflict with MIT or Apache-2.0. Two obligations do apply:

1. The OFL text must ship with the font — `Inter-OFL.txt` must stay next to it in
   any redistribution.
2. The font must not be sold on its own, and a modified version must not keep the
   reserved name "Inter".

`assets/Inter.ttf` is also compiled into the demos and the test binaries via
`include_bytes!`, which counts as redistribution: anything shipped from this repo
carries the same obligation.

If that is unwelcome, stop bundling a font and have the host supply one.
`FontRasterizer` already makes the rasteriser pluggable, so this is a packaging
decision rather than a code one.

## Dependencies

No vendored third-party source; everything is a normal crates.io crate. Their
licences matter because they travel with anything you ship.

| Crate | Licence | Feature |
|---|---|---|
| `bytemuck` | Zlib OR Apache-2.0 OR MIT | always |
| `fontdue` | MIT OR Apache-2.0 OR Zlib | `fontdue` (default) |
| `serde` | MIT OR Apache-2.0 | `serde` |
| `toml` | MIT OR Apache-2.0 | `theme-toml` (default) |
| `rustybuzz` | MIT | `shape` (off) |
| `self_cell` | **Apache-2.0 OR GPL-2.0-only** | `shape` (off) |

Every one of those offers a permissive option, so none of them constrains
libgui's own licence or forces copyleft on anything downstream.

**`self_cell` is the one worth knowing about.** It is the only dependency with no
MIT option: taking it permissively means taking it under Apache-2.0. So an
adopter who chose libgui *specifically* to stay MIT-only acquires an Apache-2.0
obligation — attribution, the NOTICE convention, the patent terms — the moment
they enable `shape`. That is a small thing, and it is exactly the kind of small
thing a legal review raises late.

Three ways out, if it matters to you:

- **Leave `shape` off.** It is off by default, and the dependency disappears with
  it. Latin-script UIs lose nothing.
- **Accept Apache-2.0.** libgui already offers Apache-2.0 as one of its own two
  options, so for most adopters this changes nothing at all.
- **Drop `self_cell`.** It exists only to hold the font bytes beside the
  `rustybuzz::Face` that borrows them. The alternative is re-parsing the face on
  each shaping call, measured at **11.1 µs** against shaping's own 10.5 µs — so
  roughly double, paid on shaped-run cache misses only, never on a settled frame.
  Whether that is worth a licence line is a judgement call, not an obvious win.
