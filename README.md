# topclock

A minimal always-on-top clock for Windows. It draws `HH:MM` directly onto the
desktop with no window frame, no backdrop, and no padding -- just the digits,
floating wherever you put them.

The window is sized to the exact pixel bounds of the glyphs, so it can sit flush
against a screen edge, and its transparent area is click-through: everything
behind the clock stays usable, and the digits themselves are the only thing you
can grab.

## Requirements

- Windows
- Rust 1.85 or newer (the crate uses edition 2024; developed on 1.98)

The only dependency is the `windows` crate, pinned to 0.58 because windows-rs
shifts parameter types between versions.

## Build

```
cargo build --release
```

The result is a single self-contained executable at
`target\release\topclock.exe`, about 172 KB. The release profile optimizes for
size (`opt-level = "z"`, LTO, one codegen unit, symbols stripped, panics abort);
roughly 38 KB of the total is the embedded icon.

For a debug build, use `cargo build` and run `target\debug\topclock.exe`.

## Icon

Rust has no icon support of its own. On Windows the icon lives in the
executable's `.rsrc` section as an ordinary Win32 resource, and Explorer
displays whichever icon has the lowest resource ID. `build.rs` embeds
`assets\topclock.ico` using the `winresource` build-dependency, which generates
the `.rc`, locates the Windows SDK's `rc.exe` and emits the link flags. Nothing
from that crate ends up in the binary; only the icon bytes do.

If the `.ico` is missing or no resource compiler is installed, the build prints
a warning and produces an executable with the default icon rather than failing.

This is the *executable* icon, seen in Explorer, on shortcuts and when pinned.
It is unrelated to the window icon (`WNDCLASSW::hIcon`), which would be pointless
here: the clock is a `WS_EX_TOOLWINDOW` with no titlebar and no taskbar or
Alt-Tab presence.

### Regenerating it

`tools\make_icon.py` draws the icon from the same colors and font as the clock,
so the two stay in step if you change the palette. It needs Pillow.

```
python tools\make_icon.py
```

It writes six sizes. Each is drawn at its own scale rather than downsampled from
one bitmap, so the 16px entry stays legible: sizes from 32px up show two rows of
digits, and 16px shows one. The 64px and larger entries are PNG-compressed,
which Windows has supported since Vista -- as uncompressed DIBs, the 128px entry
alone would be 67 KB, more than half the executable.

## Run

Launch the executable. There is no console window and no tray icon. The clock
appears in the top-right corner of the primary screen.

| Action | Result |
| --- | --- |
| Left-click and drag *on the digits* | Move the clock |
| Right-click *on the digits* | Quit |
| Escape | Quit (the window must have focus -- click it first) |

Dragging and clicking only register on the digits. The transparent space around
and between them passes the mouse through to whatever is underneath, so the
glyph strokes are the target. Grabbing a stroke is more reliable than aiming at
the middle of the colon.

## Configuration

On first run, topclock writes `topclock.ini` with the defaults and a comment for
every key. Edit it and restart the clock; the file is read once at startup.

Delete the file to regenerate it.

### Where the ini lives

The location depends on where the executable is installed, because a program
under `Program Files` cannot write next to itself without elevation. topclock
takes the first of these that applies:

1. The path in the `TOPCLOCK_CONFIG` environment variable, if set.
2. `topclock.ini` next to the executable, if it already exists.
3. `%APPDATA%\topclock\topclock.ini`, if it already exists.
4. Otherwise it creates one: next to the executable if that folder is writable,
   and in `%APPDATA%\topclock\` if it is not.

So a portable copy unzipped into a writable folder keeps its settings alongside
it, while an installed copy under `Program Files` puts them in the per-user
location automatically, with no elevation and nothing to configure.

### Notes for installers

Both installation layouts work, so pick on other grounds:

- **Per-user, into `%LocalAppData%\Programs\topclock`.** No elevation at any
  point, which suits a single-user desktop utility. This is what editors like
  VS Code default to. The ini lands beside the executable.
- **Machine-wide, into `Program Files`.** Needs elevation to install, but each
  user then gets their own settings in `%APPDATA%` on first run.

An installer should not place a `topclock.ini` into `Program Files`. The app
would find it (rule 2) and use it, but the user could not edit it without
elevation, and their per-user file would never be consulted. Ship no ini and let
the first run create one in the right place.

```ini
[clock]
fg = #00E6AA
bg = #0C0C10
bg_alpha = 0

font = Consolas
font_size = 26
weight = 700

pad = 0

margin_x = 0
margin_y = 0
```

| Key | Default | Meaning |
| --- | --- | --- |
| `fg` | `#00E6AA` | Digit color, written `#RRGGBB` |
| `bg` | `#0C0C10` | Backdrop color. Only visible when `bg_alpha > 0`, but it is also what the antialiased glyph edges blend toward, so keep it near your intended backdrop |
| `bg_alpha` | `0` | Backdrop opacity, 0-255. See the note below |
| `font` | `Consolas` | Any installed font family |
| `font_size` | `26` | Cell height in pixels. Digits render at roughly half of it, and the window resizes to fit |
| `weight` | `700` | 400 is normal, 700 is bold |
| `pad` | `0` | Transparent border around the digits, in pixels |
| `margin_x` | `0` | Gap from the right screen edge, at startup only |
| `margin_y` | `0` | Gap from the top screen edge, at startup only |

Colors are ordinary `#RRGGBB`. Win32 actually wants `COLORREF`, which is
`0x00BBGGRR`, but that flip is handled internally and never surfaces in the ini.

A value that fails to parse keeps its default rather than stopping the clock, so
a typo in a color costs you nothing. Comments must be on their own line;
trailing comments after a value are not stripped.

### The bg_alpha tradeoff

`bg_alpha = 0` is the default and does two things at once: it makes the backdrop
fully transparent, and it makes those pixels click-through, because Windows
hit-tests a layered window against its alpha channel. That is what confines
dragging to the digits.

Any value above 0 makes the entire window rectangle solid to the mouse. It also
gives you a visible plate: around 40 reads as smoked glass and helps legibility
over a bright desktop, while 120 is clearly a translucent tile.

### Why there is no width or height setting

The window measures itself from the glyphs (see below), so a fixed width or
height would only reintroduce the transparent padding the design removes -- and
with it a band of invisible mouse-catching area around the digits. `font_size`
and `pad` are the size controls.

## How it works

Four things are load-bearing.

**A layered window with per-pixel alpha.** The window is created with
`WS_EX_LAYERED` and its pixels are pushed with `UpdateLayeredWindow` and
`ULW_ALPHA` rather than painted in a `WM_PAINT` cycle. That is what allows a
genuinely transparent background with opaque digits on top. The alternative,
color-key transparency via `LWA_COLORKEY`, would also make the clock
click-through but offers no control over it and no antialiasing.

**The window is measured, not declared.** Font metrics only describe the *cell*
box, where ascent, internal leading and descent all reserve room for accents and
descenders that `12:34` never uses. That reserved space is exactly the padding
we want gone. So at startup topclock draws `88:88` onto a scratch canvas, scans
for the pixels that carry ink, and takes the true bounding box. The window
becomes that box, and the text draw origin is shifted to compensate -- in the
default configuration that means drawing five pixels above the window's own top
edge to cut the leading.

The default configuration comes out at 58x16 pixels. The sample is `88:88`
rather than the current time, so the box is stable: the clock never resizes or
drifts as the digits change. The cost is that narrow digits such as `1` do not
quite reach the left edge.

**Color comes from a mask, not from the drawing.** GDI text drawing ignores the
alpha channel, so drawing straight into a transparent buffer produces invisible
digits. topclock instead draws the text white-on-black to get a coverage mask,
then tints that mask with `fg` and `bg` and derives alpha from it. Deriving
coverage from a fixed white-on-black pass, rather than from the configured
colors, is what keeps every palette working -- comparing against the real colors
would collapse to zero coverage, and an invisible clock, for any pair of colors
sharing a channel value.

Because coverage is a smooth ramp, antialiased glyph edges fade to transparent
instead of carrying a dark fringe from the backdrop color.

**Grayscale antialiasing, not ClearType.** Subpixel antialiasing is computed for
a known background, and here the background is whatever happens to be behind the
window. The font is created with `ANTIALIASED_QUALITY` so edges stay neutral.

### Redraw behavior

A one-second timer runs, but the display only changes once a minute, so the
timer compares against the last rendered minute and skips the other 59 redraws.

## Design notes

Seconds were removed deliberately -- a ticking seconds field is distracting in a
clock meant to sit in your peripheral vision.

Double-click to cycle through colors was considered and dropped once the ini
existed. It would have needed `CS_DBLCLKS` on the window class, without which
`WM_LBUTTONDBLCLK` is never sent at all, and it would have conflicted with
dragging: `WM_LBUTTONDOWN` hands straight off to `HTCAPTION`, entering a modal
move loop that swallows the second click.
