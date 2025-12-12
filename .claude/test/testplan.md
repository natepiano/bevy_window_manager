# Manual Test Plan

## Issue Index

Tests are keyed to these tracked issues:

| Key | Issue | Platform | Description |
|-----|-------|----------|-------------|
| W1 | [winit #2645](https://github.com/rust-windowing/winit/issues/2645) | macOS | Coordinate handling broken for multi-monitor with different scale factors. Winit divides coordinates by launch monitor's scale. |
| W2 | [winit #4041](https://github.com/rust-windowing/winit/issues/4041) | Windows | DPI change causes window bounce/resize when dragging between mixed-DPI monitors. Fix in [winit #4341](https://github.com/rust-windowing/winit/pull/4341). |
| W3 | [winit #4296](https://github.com/rust-windowing/winit/issues/4296) | Windows | Invisible border offset causes reported positions to be outside monitor bounds for maximized/snapped windows. |
| W4 | [winit #4107](https://github.com/rust-windowing/winit/issues/4107) | Windows | `GetWindowRect` includes invisible resize border (~7-11px), causing negative positions for snapped windows. |
| W5 | [winit #3124](https://github.com/rust-windowing/winit/issues/3124) | Windows | Exclusive fullscreen crashes on startup with DX12 due to DXGI flip model limitations. |
| B1 | [bevy #5485](https://github.com/bevyengine/bevy/issues/5485) | Windows/Linux | Surface creation crash when entering exclusive fullscreen at startup. Related to W5. |
| B2 | [bevy #22060](https://github.com/bevyengine/bevy/issues/22060) | macOS | TLS panic on quit from exclusive fullscreen. |
| M1 | macOS size caching | macOS | macOS caches window size during scale transitions. Setting 1x1 size during restore causes incorrect size when dragging back to original monitor. |

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
- [ ] Move app window on Launch Monitor 0, resize, close
- [ ] Relaunch → app window restores to same position/size

#### W1: Cross-Monitor High→Low DPI
*Tests: HigherToLower two-phase strategy*
- [ ] Launch from Monitor 0, move app window to Monitor 1, resize, close
- [ ] Relaunch from Launch Monitor 0 → app window moves to Monitor 1 with correct size
- [ ] Validate at various dock positions (left, right, maximized)

#### M1: DPI Drag Size Stability
*Tests: macOS size caching during scale transitions*
- [ ] Launch from Monitor 0 (high scale), restore to Monitor 1 (low scale)
- [ ] Drag app window back to Monitor 0 → size should scale correctly (2x), no jump to tiny size
- [ ] Drag app window back to Monitor 1 → size should scale correctly (0.5x)

#### Fullscreen: Borderless Green Button
- [ ] Press green button for borderless on Monitor 0, close (command-Q)
- [ ] Relaunch → restores to borderless on Monitor 0

#### B2: Fullscreen Exclusive
*Tests: TLS panic on quit*
- [ ] Move app window to Monitor 0, Press 1 for exclusive, select video mode, close (command-Q)
- [ ] Relaunch → restores to exclusive fullscreen on Monitor 0
- [ ] Verify no panic on quit

#### Fullscreen: Programmatic Borderless
- [ ] Move app window to Monitor 0, Press 2 for borderless, close
- [ ] Relaunch → restores correctly as borderless on Monitor 0

### Launch Monitor 1 Tests (Low Scale - External)

#### Restore: Same Monitor
- [ ] Move app window on Launch Monitor 1, resize, close
- [ ] Relaunch → app window restores to same position/size

#### W1: Cross-Monitor Low→High DPI
*Tests: LowerToHigher strategy*
- [ ] Launch from Monitor 1, move app window to Monitor 0, resize, close
- [ ] Relaunch from Launch Monitor 1 → app window launches on Monitor 0 with correct size and position
- [ ] Validate at various dock positions (left, right, maximized)

#### M1: DPI Drag Size Stability
*Tests: macOS size caching during scale transitions*
- [ ] Launch from Monitor 1 (low scale), restore to Monitor 0 (high scale)
- [ ] Drag app window back to Monitor 1 → size should scale correctly (0.5x)
- [ ] Drag app window back to Monitor 0 → size should scale correctly (2x)

#### Fullscreen: Borderless Green Button
- [ ] Press green button for borderless on Monitor 1, close (command-Q)
- [ ] Relaunch → restores to borderless on Monitor 1

#### B2: Fullscreen Exclusive
*Tests: TLS panic on quit*
- [ ] Move app window to Monitor 1, Press 1 for exclusive, select video mode, close (command-Q)
- [ ] Relaunch → restores to exclusive fullscreen on Monitor 1
- [ ] Verify no panic on quit

#### Fullscreen: Programmatic Borderless
- [ ] Move app window to Monitor 1, Press 2 for borderless, close
- [ ] Relaunch → restores correctly as borderless on Monitor 1

---

## Windows Tests

`using restore_window example`

### Launch Monitor 0 Tests (High Scale - Primary)

#### Restore: Same Monitor
- [ ] Move app window on Launch Monitor 0, resize, close
- [ ] Relaunch → app window restores to same position/size

#### Cross-Monitor High→Low DPI
*Tests: CompensateSizeOnly strategy*
- [ ] Launch from Monitor 0, move app window to Monitor 1, resize, close
- [ ] Relaunch from Launch Monitor 0 → app window moves to Monitor 1 with correct size

#### W3: Restore Maximized Window
*Tests: Invisible border, window center detection*
- [ ] Maximize app window on Launch Monitor 0, close
- [ ] Relaunch → restores maximized on Monitor 0 (not Monitor 1)

#### W4: Restore Snapped Window
*Tests: Invisible border offset*
- [ ] Snap app window to left half of Launch Monitor 0, close
- [ ] Relaunch → restores snapped position (not shifted by border width)

#### W2: DPI Drag
*Tests: Bounce/resize bug*
- [ ] Drag app window slowly from Monitor 0 to Monitor 1
- [ ] App window moves smoothly, no bouncing back, resizes correctly

#### Fullscreen: Borderless
- [ ] Press 2 for borderless on Monitor 0, close
- [ ] Relaunch → restores to borderless on Monitor 0

#### W5/B1: Fullscreen Exclusive
*Tests: DX12/DXGI surface creation crash*
- [ ] Press 1 for exclusive on Monitor 0, select video mode, close
- [ ] Relaunch → restores to exclusive fullscreen (brief windowed flash is expected)

### Launch Monitor 1 Tests (Low Scale - External)

#### Restore: Same Monitor
- [ ] Move app window on Launch Monitor 1, resize, close
- [ ] Relaunch → app window restores to same position/size

#### Cross-Monitor Low→High DPI
*Tests: CompensateSizeOnly strategy*
- [ ] Launch from Monitor 1, move app window to Monitor 0, resize, close
- [ ] Relaunch from Launch Monitor 1 → app window moves to Monitor 0 with correct size

#### W2: DPI Drag
*Tests: Bounce/resize bug*
- [ ] Drag app window slowly from Monitor 1 to Monitor 0
- [ ] App window moves smoothly, no bouncing back, resizes correctly

#### Fullscreen: Borderless
- [ ] Press 2 for borderless on Monitor 1, close
- [ ] Relaunch → restores to borderless on Monitor 1

#### W5/B1: Fullscreen Exclusive
*Tests: DX12/DXGI surface creation crash*
- [ ] Press 1 for exclusive on Monitor 1, select video mode, close
- [ ] Relaunch → restores to exclusive fullscreen (brief windowed flash is expected)
