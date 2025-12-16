# Manual Test Plan

## Issue Index

Tests are keyed to these tracked issues:

| Key | Issue | Platform | Feature Flag | Description |
|-----|-------|----------|--------------|-------------|
| W1 | [winit #4440](https://github.com/rust-windowing/winit/issues/4440) | macOS | `workaround-macos-scale-compensation` | `set_outer_position` and `request_inner_size` use current monitor's scale factor instead of target monitor's. When restoring to a different-scale monitor, coordinates are converted incorrectly. |
| W2 | [winit #4041](https://github.com/rust-windowing/winit/issues/4041) | Windows | `workaround-winit-4341` | DPI change causes window bounce/resize when dragging between mixed-DPI monitors. Fix in [winit #4341](https://github.com/rust-windowing/winit/pull/4341). |
| W3 | [winit #3124](https://github.com/rust-windowing/winit/issues/3124) | Windows | `workaround-winit-3124` | Exclusive fullscreen crashes on startup with DX12 due to DXGI flip model limitations. We defer fullscreen until after surface creation via `FullscreenRestoreState`. There currently is not an open issue for this in bevy - once we validate our own fix we should open a bevy issue. |
| W4 | [winit #4441](https://github.com/rust-windowing/winit/issues/4441) | macOS | `workaround-macos-drag-back-reset` | Window size resets to default when dragging between monitors with different scale factors after programmatic resize. AppKit tracks "intended size" per scale factor; programmatic `setContentSize` doesn't update this tracking, but manual resize does. |
| W5 | [winit #4443](https://github.com/rust-windowing/winit/issues/4443) | Linux X11 | `workaround-winit-4443` | On X11, keyboard snap/tile (Meta+Arrow) emits `SurfaceResized` but not `Moved` even when position changed. We query `outer_position()` directly when saving state. Related: [bevy #17576](https://github.com/bevyengine/bevy/issues/17576). |
| B1 | [bevy PR #22060](https://github.com/bevyengine/bevy/pull/22060) | macOS | `workaround-bevy-22060` | TLS panic on quit from exclusive fullscreen. We exit fullscreen during `world.clear_all()` before TLS destruction. Remove when using Bevy 0.18+. |

**Key prefixes:** W = winit issue, B = Bevy issue, M = macOS-specific (internal fix)

## Test Setup

**Monitor Configuration:**
- Launch Monitor 0 (primary): Higher scale (e.g., 2.0 / 200%)
- Launch Monitor 1 (external): Lower scale (e.g., 1.0 / 100% on Mac, 1.75 / 175% on Windows)

**Commands:**

macOS:
```bash
rm ~/Library/Application\ Support/restore_window/window_state.json
```

Windows (PowerShell):
```powershell
del $env:APPDATA\restore_window\window_state.json
```

---

## macOS Tests

`using restore_window example`

### Launch Monitor 0 Tests (High Scale - Primary)

#### Restore: Same Monitor
*Test: Basic position/size persistence*
- [x] Move app window on Launch Monitor 0, resize, close
- [x] Relaunch → app window restores to same position/size

#### Cross-Monitor High→Low DPI (W1)
*Test: HigherToLower two-phase strategy*
- [x] Launch from Monitor 0, move app window to Monitor 1, resize, close
- [x] Relaunch from Launch Monitor 0 → app window moves to Monitor 1 with correct size
- [x] Validate at various dock positions (left, right, maximized)

#### DPI Drag Size Stability (W4)
*Test: AppKit per-scale size tracking with workaround*
- [x] Launch from Monitor 0 (high scale), restore to Monitor 1 (low scale)
- [x] Drag app window back to Monitor 0 → size should scale correctly (2x), not reset to default
- [x] Drag app window back to Monitor 1 → size should scale correctly (0.5x)
- [x] Manual resize on Monitor 1, then drag back to Monitor 0 → user's size preserved

#### Fullscreen: Borderless Green Button
*Test: macOS green button borderless restoration*
- [x] Press green button for borderless on Monitor 0, close (command-Q)
- [x] Relaunch → restores to borderless on Monitor 0

#### Fullscreen: Programmatic Borderless
- [x] Move app window to Monitor 0, Press 2 for borderless, close
- [x] Relaunch → restores correctly as borderless on Monitor 0

#### Fullscreen Exclusive (B1)
*Test: TLS panic on quit*
- [x] Move app window to Monitor 0, Press 1 for exclusive, select video mode, close (command-Q)
- [x] Relaunch → restores to exclusive fullscreen on Monitor 0
- [x] Verify no panic on quit

### Launch Monitor 1 Tests (Low Scale - External)

#### Restore: Same Monitor
- [x] Move app window on Launch Monitor 1, resize, close
- [x] Relaunch → app window restores to same position/size

#### Cross-Monitor Low→High DPI (W1)
*Test: LowerToHigher strategy*
- [x] Launch from Monitor 1, move app window to Monitor 0, resize, close
- [x] Relaunch from Launch Monitor 1 → app window launches on Monitor 0 with correct size and position
- [x] Validate at various dock positions (left, right, maximized)

---

## Windows Tests

`using restore_window example`

### Launch Monitor 0 Tests (High Scale - Primary)

#### Restore: Same Monitor
- [x] Move app window on Launch Monitor 0, resize, close
- [x] Relaunch → app window restores to same position/size

#### Cross-Monitor High→Low DPI
*Test: CompensateSizeOnly strategy*
- [x] Launch from Monitor 0, move app window to Monitor 1, resize, close
- [x] Relaunch from Launch Monitor 0 → app window moves to Monitor 1 with correct size

#### Restore Maximized Window
- [x] Maximize app window on Launch Monitor 0, close
- [x] Relaunch → restores maximized on Monitor 0

#### DPI Drag (W2)
*Test: Bounce/resize bug*
- [x] Drag app window slowly from Monitor 0 to Monitor 1
- [x] App window moves smoothly, no bouncing back, resizes correctly

#### Fullscreen: Borderless
- [x] Press 2 for borderless on Monitor 0, close
- [x] Relaunch → restores to borderless on Monitor 0

#### Fullscreen Exclusive (W3)
*Test: DX12/DXGI surface creation crash*
- [x] Press 1 for exclusive on Monitor 0, select video mode, close
- [x] Relaunch → restores to exclusive fullscreen (brief windowed flash is expected)

### Launch Monitor 1 Tests (Low Scale - External)

#### Restore: Same Monitor
- [x] Move app window on Launch Monitor 1, resize, close
- [x] Relaunch → app window restores to same position/size

#### Cross-Monitor Low→High DPI
*Test: CompensateSizeOnly strategy*
- [x] Launch from Monitor 1, move app window to Monitor 0, resize, close
- [x] Relaunch from Launch Monitor 1 → app window moves to Monitor 0 with correct size

#### DPI Drag (W2)
*Test: Bounce/resize bug*
- [x] Drag app window slowly from Monitor 1 to Monitor 0
- [x] App window moves smoothly, no bouncing back, resizes correctly

#### Fullscreen: Borderless
- [x] Press 2 for borderless on Monitor 1, close
- [x] Relaunch → restores to borderless on Monitor 1

#### Fullscreen Exclusive (W3)
*Test: DX12/DXGI surface creation crash*
- [x] Press 1 for exclusive on Monitor 1, select video mode, close
- [x] Relaunch → restores to exclusive fullscreen (brief windowed flash is expected)

---

## Linux Wayland Tests

`using restore_window example`

**Setup:**
```bash
rm ~/.local/share/restore_window/window_state.json
# Ensure Wayland is running (default on modern KDE/GNOME)
```

**Note:** On Wayland, clients cannot query or set window position. Position is always `Automatic`. Only size and fullscreen state can be restored.

### Single Monitor Tests

#### Restore: Size Only
*Test: Size persistence (position not available on Wayland)*
- [ ] Resize app window, close
- [ ] Relaunch → app window restores to same size (position determined by compositor)

#### Fullscreen: Borderless
- [ ] Press 2 for borderless, close
- [ ] Relaunch → restores to borderless fullscreen

#### Fullscreen: Exclusive
- [ ] Press 1 for exclusive, select video mode, close
- [ ] Relaunch → restores to exclusive fullscreen

---

## Linux X11 Tests

`using restore_window example`

**Setup:**
```bash
rm ~/.local/share/restore_window/window_state.json
# Force X11 session:
WAYLAND_DISPLAY= cargo run --example restore_window
```

**Note:** On X11, keyboard snap/tile operations may not emit `Moved` events (W5). Our save code queries `outer_position()` directly to work around this.

### Single Monitor Tests

#### Restore: Position and Size
*Test: Basic position/size persistence*
- [ ] Move app window, resize, close
- [ ] Relaunch → app window restores to same position/size

#### Keyboard Snap Position (W5)
*Test: Position saved correctly after keyboard snap*
- [ ] Use keyboard snap (KDE: Meta+Arrow) to tile window
- [ ] Close app
- [ ] Relaunch → app window restores to snapped position

#### Drag Position
*Test: Position saved correctly after drag*
- [ ] Drag app window to new position, close
- [ ] Relaunch → app window restores to dragged position

#### Fullscreen: Borderless
- [ ] Press 2 for borderless, close
- [ ] Relaunch → restores to borderless fullscreen

#### Fullscreen: Exclusive
- [ ] Press 1 for exclusive, select video mode, close
- [ ] Relaunch → restores to exclusive fullscreen
