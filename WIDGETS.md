# What real applications use, and what libgui has

A survey, done to answer one question: if someone sat down to build a
professional desktop application on libgui tomorrow, what would they reach for
and not find?

## Method, and its limits

Fourteen applications were chosen to span the four bands libgui claims to
serve, and their interfaces were enumerated element by element. The libgui
column is not a judgement — it was taken from the source, by listing the public
widget surface in `widgets.rs`, `text_edit.rs`, `table.rs`, `dock.rs` and the
container calls on `Ui`.

Two honest limits. This is a reading of these applications' interfaces from
knowledge of them, not a fresh teardown of each one with a profiler attached;
an element used in one obscure dialog may be missed. And "used by N apps" is a
weak signal on its own — a scroll bar appears in all fourteen and needs no
argument, while a colour picker appears in six and is load-bearing in every one
of them. The counts inform the priority; they do not set it.

The applications:

| Band | Applications |
|---|---|
| CAD / 3D | Fusion 360, SolidWorks, Blender, FreeCAD |
| Creative / media | Figma, DaVinci Resolve, Ableton Live, Photoshop |
| Developer tools | VS Code, Xcode |
| Consumer / productivity | Slack, Notion, macOS System Settings, Spotify |

## The census

`yes` = present and complete. `part` = present but short of what the surveyed
apps do with it. `no` = absent.

### Text and structure

| Element | Apps | libgui |
|---|---|---|
| Label, heading, section header | 14 | yes |
| Body paragraph, wrapped | 14 | yes |
| Separator / rule | 14 | yes |
| Inline link in text | 9 | **no** |
| Rich text (bold/italic spans in one run) | 7 | **no** |
| Rotated / angled text | 6 | **no** — see *The constraint underneath* |

### Actions

| Element | Apps | libgui |
|---|---|---|
| Button, primary button | 14 | yes |
| Icon-only button | 14 | yes (`button_styled` + custom paint) |
| Toolbar | 13 | part — a row of buttons works; **overflow does not** |
| Menu bar, menus, submenus, shortcuts in menus | 14 | yes |
| Context menu | 14 | yes |
| Split button (action + dropdown arrow) | 8 | **no** |
| Command palette | 6 | **no** — composable from popup + filtered list |

### Input controls

| Element | Apps | libgui |
|---|---|---|
| Checkbox, radio, toggle | 14 | yes |
| Slider (h and v) | 12 | yes |
| Drag-to-change number field | 9 | yes (`drag_value`) |
| Numeric field with units and expressions | 9 | yes — `validated_input` with the app's own evaluator; `libgui_units` for an app without one |
| Single-line text field | 14 | yes |
| Multi-line text area | 12 | yes |
| Dropdown / combo | 14 | yes |
| Segmented control | 11 | yes |
| Searchable / filterable dropdown | 9 | **no** |
| Search field with clear affordance | 13 | part — `text_input` plus your own button |
| **Colour picker** | 10 | **no** |
| Date / time picker | 5 | **no** |
| File path field with a browse button | 11 | part — the field is yours; libgui opens no dialogs, by design |

### Containers and navigation

| Element | Apps | libgui |
|---|---|---|
| Scroll area | 14 | yes |
| Panel, card | 14 | yes |
| Collapsible section / disclosure | 14 | yes (`collection`) |
| Tabs | 14 | yes (dock) |
| Dockable, tear-off panels | 9 | yes |
| Splitter / resizable panes | 13 | yes |
| Tree view | 12 | yes (`tree_row`) |
| **Virtualised tree** | 6 | **no** — `virtual_list` is flat |
| Breadcrumb | 8 | **no** |
| Status bar | 12 | part — a container; no widget, and none needed |
| Sidebar / rail navigation | 10 | yes (containers) |

### Data display

| Element | Apps | libgui |
|---|---|---|
| Table with sortable, resizable columns | 12 | yes |
| Frozen columns | 7 | yes |
| Editable table cells | 8 | part — the cell callback takes `&mut Ui`, so a field in a cell works; the *editing behaviour* (click to edit, Enter commits, Escape cancels, Tab to the next cell) is yours |
| 2-D cell cursor (arrow keys between cells) | 7 | **no** |
| Virtualised list | 11 | yes |
| Progress bar, determinate and indeterminate | 14 | yes |
| **Chart with axes, series, legend** | 8 | **no** — `plot` is a sparkline |
| Sparkline / meter | 9 | yes (`plot`) |
| Badge / count pill | 10 | **no** — trivial from a painted leaf |

### Overlays and feedback

| Element | Apps | libgui |
|---|---|---|
| Tooltip | 14 | yes |
| Popup / dropdown surface | 14 | yes |
| Modal dialog | 14 | part — **deliberately**: built from layers, see DESIGN §13 |
| **Toast / notification** | 12 | **no** |
| Inline validation error on a field | 11 | yes (`validated_input`) |
| Empty state | 12 | part — containers and a label; no widget needed |
| Drag and drop with a drag preview | 11 | yes |

### Keyboard

Not widgets, but the survey kept meeting them, and an element you cannot reach
from the keyboard is missing for a portion of every application's users.

| Element | Apps | libgui |
|---|---|---|
| Tab traversal, focus ring | 14 | yes |
| Focus scrolled into view | 14 | yes |
| Arrow-key navigation within a list or tree | 14 | yes (`collection`) |
| **Arrow-key navigation within a menu** | 14 | **no** |
| **Type-ahead in a list** (type "br" to jump to "bracket") | 12 | **no** |
| Shift+Arrow to extend a selection | 11 | **no** — multi-select is pointer-only |
| Application shortcuts, chords | 14 | yes (`libgui_keymap`) |
| Screen reader / platform accessibility tree | 14 | **no** — deferred, DESIGN §14 |

### Canvas-shaped surfaces

| Element | Apps | libgui |
|---|---|---|
| Pan/zoom canvas | 9 | yes |
| 3D viewport embedded in the UI | 6 | yes |
| Node graph | 5 | yes (canvas + panels) |
| **Timeline / ruler with a scrubber** | 6 | **no** |
| Selection handles / gizmos on canvas objects | 8 | app-level, and rightly so |

## The constraint underneath

One finding is not a widget, and matters more than most of the ones above.

`Instance` is an axis-aligned rect: `rect`, `uv`, `color`, `border_color`,
`clip`, `params`. There is no angle anywhere in it. Vector geometry escapes
this, because `polyline` and `bezier` take points the app computes and can
rotate itself — that is how the spinning-gear test works. **Text and images
cannot be rotated at all.**

For a CAD sketcher that is not a cosmetic limit. A dimension annotation along
an angled edge is the ordinary case, not the exotic one, and the same gap stops
a rotated axis label on a chart and vertical text in a timeline. Six of the
fourteen applications rotate text somewhere.

Closing it means a field in `Instance`, which moves `INSTANCE_STRIDE`, bumps
`CONTRACT_VERSION`, and touches every backend and the conformance kit. That is
why it belongs in this document rather than in a list of widgets to write: it
is the one gap whose cost is paid by everyone who has already written a
backend, so it wants deciding deliberately and early rather than late.

## What the survey actually says

Three things, beyond the table.

**The gaps are concentrated in data entry, not in layout.** Containers, tabs,
docking, trees, tables, scrolling and virtualisation are all present and
complete. What is missing clusters almost entirely around *typing a value in*:
colour, units, dates, expressions, filtered choice, and telling the user their
input was wrong. An application built on libgui today can be structured
properly and then struggles at the leaves.

**Several "missing" entries are compositions, not primitives.** A command
palette is a popup, a text field and a filtered list. A badge is a painted
leaf. A search field is a text input and a button. These are cheap, and the
argument for adding them is consistency rather than capability — every app
writing its own means fourteen slightly different ones.

**Two are neither, and are the real work.** A colour picker needs a
saturation/value square, a hue strip, hex and channel entry, and an eyedropper
the host must supply. A chart needs axis ticks, label collision avoidance and a
legend. Neither composes out of what exists.

## Recommended order

Sized in days, and ordered by what unblocks vCAD first, since that is the
application actually being built.

| # | Item | Why now | Rough size |
|---|---|---|---|
| 1 | ~~Numeric field with units and expressions~~ | **Done**, as #3: the app's evaluator behind `validated_input`. An evaluator in core was tried and withdrawn — see DESIGN §13. | — |
| 2 | Colour picker | Layer and appearance colour. Ten of fourteen apps; `to_hex`/`parse_hex` already exist, so the model is half-built. | 4–5 d |
| 3 | ~~Inline field validation~~ | **Done** — `validated_input`: commit, cancel, refused text kept with its reason, caret at the problem. | — |
| 4 | Toast / notification | The standard way to report a non-modal failure. Twelve of fourteen. | 2 d |
| 5 | **Decide on rotation** | Not build — decide. The cost falls on backend authors, and it gets worse the longer it waits. | — |
| 6 | Virtualised tree | A CAD assembly browser is a tree with tens of thousands of nodes. Today it is a tree or it is virtual, not both. | 3 d |
| 7 | Menu arrow keys, list type-ahead, Shift+Arrow selection | Keyboard gaps in widgets that otherwise exist. Small, and they compound with accessibility later. | 3 d |
| 8 | Searchable dropdown, command palette, search field, badge, split button | The compositions. Cheap, and worth having once rather than five times. | 4 d total |
| 9 | Table cell editing behaviour + 2-D cursor | A parameters table. A field in a cell already works; what is missing is click-to-edit, commit/cancel, and Tab moving to the next cell. | 4 d |
| 10 | Chart with axes | No CAD need. Do it when an app asks. | 4 d |
| 11 | Breadcrumb, timeline/ruler, date picker, rich text, inline links | No current demand from any planned application. | — |

Items 1–4 are about a fortnight and close the cluster the survey identifies.
Item 5 costs nothing to decide and gets more expensive to defer.

## What was deliberately not counted as a gap

Some absences are decisions, recorded in DESIGN.md, and were excluded rather
than listed as missing:

- **No modal primitive.** What a modal blocks is the app's question. Layers are
  the mechanism.
- **No file dialogs, no clipboard, no window creation.** libgui does not touch
  the platform; the host supplies these.
- **No icon set.** Icons are the application's identity. The painter draws
  vectors and the host supplies textures.
- **No keybindings in core.** Platform convention lives in `libgui_keymap` or
  the app.
- **No layout DSL or styling language.** Themes are TOML; layout is code.
