# Licensing — decision pending

No licence has been chosen yet, so **libgui is currently "all rights reserved"**:
nobody can legally use, fork or depend on it until a licence is added. This file
records what needs deciding.

## The code

The intent is open source under MIT. The one thing worth deciding first is
whether to use **MIT alone** or the Rust ecosystem's usual **`MIT OR Apache-2.0`**.

| | MIT only | MIT OR Apache-2.0 |
|---|---|---|
| Simplicity | One short file everyone knows | Two files, one extra line in each manifest |
| Ecosystem fit | Fine, common enough | The de-facto default: `rustc`, `serde`, `wgpu`, `winit`, `egui` |
| Patent grant | None | Apache-2.0 grants patent rights explicitly |
| Corporate adoption | Some legal teams ask for a patent grant | Removes that objection |

For a library aimed at professional tools, where adopters are often companies,
the dual licence is the lower-friction choice and costs almost nothing. MIT alone
is entirely reasonable if you would rather keep it simple. Either way the code
stays permissively open source — this is not a copyleft question.

Whichever is chosen:

- add `LICENSE-MIT` (and `LICENSE-APACHE` if dual),
- add `license = "MIT"` or `license = "MIT OR Apache-2.0"` to `[workspace.package]`,
  which every crate already inherits from,
- state it in `README.md`.

## The bundled font

`assets/Inter.ttf` is **not** covered by the code licence. Inter is under the SIL
Open Font License 1.1, whose terms are already kept alongside it in
`assets/Inter-OFL.txt`.

The OFL does not affect the licence of software that merely bundles the font, so
there is no conflict with MIT or Apache-2.0. Two obligations do apply and are
easy to miss:

1. The OFL text must ship with the font — it does, and `Inter-OFL.txt` must stay
   next to it in any redistribution.
2. The font must not be sold on its own, and a modified version must not keep the
   reserved name "Inter".

`assets/Inter.ttf` is also compiled into the demo and the test binaries via
`include_bytes!`, which counts as redistribution: anything shipped from this
repo carries the same obligation.

If that is unwelcome, the alternative is to stop bundling a font and have the
host supply one. `FontRasterizer` already makes the rasteriser pluggable, so this
is a packaging decision rather than a code one.

## Third-party code

No vendored third-party source. Dependencies are normal crates.io crates
(`fontdue`, `serde`, `toml`, `naga`, `wgpu`, `winit`, `bytemuck`, `pollster`),
all MIT or MIT/Apache-2.0, so none of them constrains the choice above.
