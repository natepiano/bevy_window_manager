# Manual Test Plan

## Guidelines for Adding Platform Tests

When adding a new platform (e.g., Linux):
1. Identify platform-specific workarounds in the codebase (search for `#[cfg(target_os = "...")]`)
2. Reference the linked issues in code comments to understand what each workaround fixes
3. Add a platform section below with tests covering: restore scenarios, fullscreen modes, and any platform-specific bugs
4. Keep tests minimal - focus on the specific issues the code addresses

## Test Setup

**Monitor Configuration:**
- Monitor 0 (primary): Higher scale (e.g., 2.0 / 200%)
- Monitor 1 (external): Lower scale (e.g., 1.0 / 100% on Mac, 1.75 / 175% on Windows)

**Commands:**
```bash
# Delete saved state to start fresh
# macOS:
rm ~/Library/Application\ Support/simple_restore/windows.ron
rm ~/Library/Application\ Support/fullscreen_modes/windows.ron

# Windows (PowerShell):
del $env:APPDATA\simple_restore\windows.ron
del $env:APPDATA\fullscreen_modes\windows.ron
```

---

## macOS Tests

`using simple_restore example`

### Restore: Same Monitor (Monitor 0 → 0)
- [x] Move window on Monitor 0, resize, close
- [x] Relaunch → window restores to same position/size
- [x] Repeat for Monitor 1 -> 1

### Restore: Cross-Monitor Low→High DPI (Monitor 1 → 0)
*Tests: winit #2645 coordinate bug, LowerToHigher strategy*
- [x] Move launch window to Monitor 1, resize window on Monitor 0, close - without compensation this would not restore at correct size
- [x] Relaunch (launches on Monitor 1) → window launches on Monitor 0 with correct size and position
- [x] Validate at various dock positions (left, right, maximized)

### Restore: Cross-Monitor High→Low DPI (Monitor 0 → 1)
*Tests: winit #2645, HigherToLower two-phase strategy*
- [x] Move launch window to Monitor 0, move test window to monitor 1 resize, close
- [x] Relaunch from Monitor 0 → window moves to Monitor 1 with correct size
- [x] Validate at various dock positions (left, right, maximized)

### Fullscreen: Borderless - MacOS Green Button
- [x] Press green button for borderless on Monitor 0, close (command-Q)
- [x] Relaunch → restores to borderless on Monitor 0
- [x] Repeat on Monitor 1

`using fullscreen_modes example`
### Fullscreen: Exclusive
- [ ] move to Monitor 0, Press 1 for exclusive, select video mode, close (command-Q)
- [ ] Relaunch → restores to exclusive fullscreen
- [ ] Repeat on Monitor 1
- **Note** this also tests bevy #22060 TLS panic - if it doesn't panic on quit, we're good

### Fullscreen: programmatic Borderless
- [ ] Move to Monitor 0, Press 2 for borderless, close
- [ ] Relaunch → restores correctly programatically set as borderless launch
- [ ] Repeat for Monitor 1

---

## Windows Tests

### Restore: Same Monitor (Monitor 0 → 0)
- [ ] Move window on Monitor 0, resize, close
- [ ] Relaunch → window restores to same position/size

### Restore: Cross-Monitor Different Scales
*Tests: CompensateSizeOnly strategy*
- [ ] Move window to Monitor 1, resize, close
- [ ] Relaunch → window moves to Monitor 1 with correct size (not shrunk/grown)

### Restore: Maximized Window
*Tests: winit #4296 invisible border, window center detection*
- [ ] Maximize window on Monitor 0, close
- [ ] Relaunch → restores maximized on Monitor 0 (not Monitor 1)

### Restore: Snapped Window
*Tests: winit #4107 invisible border offset*
- [ ] Snap window to left half of Monitor 0, close
- [ ] Relaunch → restores snapped position (not shifted by border width)

### DPI Drag Fix
*Tests: winit #4041 bounce/resize bug*
- [ ] Drag window slowly from Monitor 0 to Monitor 1
- [ ] Window moves smoothly, no bouncing back, resizes correctly

### Fullscreen: Borderless
- [ ] Press B for borderless, close
- [ ] Relaunch → restores to borderless

### Fullscreen: Exclusive (Startup Delay)
*Tests: winit #3124, bevy #5485 surface creation crash*
- [ ] Press F for exclusive, select video mode, close
- [ ] Relaunch → restores to exclusive fullscreen (brief windowed flash is expected)

### Fullscreen: Cross-Monitor Restore
- [ ] Enter fullscreen on Monitor 1, close
- [ ] Relaunch → restores fullscreen on Monitor 1 (not Monitor 0)
