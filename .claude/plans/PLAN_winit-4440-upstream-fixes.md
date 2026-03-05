# Upstream Fixes for winit #4440: Cross-Monitor Scale Factor Bug

## Overview

Two coordinated fixes that together eliminate the need for bevy_window_manager's
`workaround-winit-4440` feature flag:

1. **winit fix**: `set_outer_position` resolves the TARGET monitor's scale factor
   instead of using the current window's scale factor
2. **bevy fix**: process position changes BEFORE size changes so `request_inner_size`
   sees the correct `scale_factor()` after the window has moved

Neither fix is useful alone. Without the winit fix, the window lands on the wrong
monitor so bevy's ordering doesn't matter. Without the bevy reorder, the window is
resized on the wrong monitor before being moved.

## Current State

- Both fixes implemented and tested on macOS (2x retina + 1x external)
- Debug logging in bevy_winit/system.rs (cross-platform) confirms ordering
- Debug logging in winit's macOS window_delegate.rs confirms scale resolution
- winit changes: `src/monitor.rs` (shared algorithm + tests), plus platform fixes
  in macos/window_delegate.rs, windows/window.rs, linux/x11/window.rs
- bevy changes: `crates/bevy_winit/src/system.rs` (position block moved above size block)
- Both repos have uncommitted changes on detached HEADs (winit 0.30.13, bevy 0.18.1)
- Fork remotes added: `fork` -> natepiano/winit, `fork` -> natepiano/bevy

## Phase 1: Push Fix Branches to natepiano Forks

### winit

```bash
cd ~/rust/winit
git checkout -b fix/set-outer-position-scale-factor
# commit all changes
git push fork fix/set-outer-position-scale-factor
```

Changes to commit:
- `src/monitor.rs` — `MonitorBounds`, `resolve_scale_factor()`, 16 unit tests
- `src/platform_impl/macos/window_delegate.rs` — `scale_factor_for()`, debug logging
- `src/platform_impl/windows/window.rs` — `scale_factor_for()`, fixed `set_outer_position`
- `src/platform_impl/linux/x11/window.rs` — `scale_factor_for()`, fixed `set_outer_position`

### bevy

Before committing bevy, update its `[patch.crates-io]` to use the git fork instead
of a local path so the branch is self-contained:

```toml
[patch.crates-io]
winit = { git = "https://github.com/natepiano/winit", branch = "fix/set-outer-position-scale-factor" }
```

```bash
cd ~/rust/bevy
git checkout -b fix/position-before-size
# commit all changes (system.rs + Cargo.toml patch)
git push fork fix/position-before-size
```

Changes to commit:
- `Cargo.toml` — winit patch pointing to natepiano fork branch
- `crates/bevy_winit/src/system.rs` — position block moved before size block + debug logging

### bevy_window_manager

Update `Cargo.toml` `[patch.crates-io]` to use git dependencies:

```toml
[patch.crates-io]
winit = { git = "https://github.com/natepiano/winit", branch = "fix/set-outer-position-scale-factor" }
bevy = { git = "https://github.com/natepiano/bevy", branch = "fix/position-before-size" }
bevy_internal = { git = "https://github.com/natepiano/bevy", branch = "fix/position-before-size" }
# ... all other bevy sub-crate patches with same git source
```

Verify it builds on macOS, then push.

## Phase 2: Cross-Platform Testing (Windows + Linux)

### Setup on each platform

```bash
git clone https://github.com/natepiano/bevy_window_manager
cd bevy_window_manager
git checkout <branch-with-git-patches>
cargo build --example restore_window --no-default-features
```

### Test procedure

The `--no-default-features` flag disables ALL workarounds including `workaround-winit-4440`,
so bevy_window_manager passes position and size straight through with `ApplyUnchanged`.
If the upstream fixes work, the window should restore at the correct position AND size
on a different-scale monitor.

**Required: two monitors with different scale factors** (e.g. 1x + 1.5x, or 1x + 2x).

1. Launch:
   ```
   RUST_LOG=warn,bevy_winit::system=debug cargo run --example restore_window --no-default-features
   ```

2. Drag window to the HIGH scale monitor, close the app (press Q or close button)

3. Relaunch FROM the LOW scale monitor (terminal on the low-scale monitor)

4. Observe:
   - Window should appear on the high-scale monitor at the saved position and size
   - Logs should show `winit_scale_before=<low>` then `winit_scale_after=<high>`
   - `request_inner_size` should show `winit_scale=<high>` (the target monitor's scale)

5. Repeat in reverse: drag to LOW scale monitor, close, relaunch from HIGH scale monitor

6. Verify both directions produce correct position and size

### What success looks like

```
[bevy_winit] set_outer_position: position=Physical(...), winit_scale_before=1
[bevy_winit] set_outer_position done: winit_scale_after=2
[bevy_winit] resolution changed: ... winit_scale=2
[bevy_winit] scale factor unchanged, using physical_size as-is
[bevy_winit] calling request_inner_size(Physical(...)), winit_scale=2
```

The critical line: `winit_scale_after` matches `winit_scale` in the `request_inner_size` call,
and both reflect the TARGET monitor's scale factor, not the launch monitor's.

### What failure looks like

- Window appears at wrong position (winit fix not working on that platform)
- Window appears at wrong size — half or double (bevy reorder not taking effect,
  or `request_inner_size` using wrong scale)
- `winit_scale_after` still shows the launch monitor's scale (position fix didn't
  actually move the window to the right monitor)

### Platform-specific notes

**Windows**: The winit fix uses `monitor::available_monitors()` which calls Win32
`EnumDisplayMonitors`. Debug logging is only in bevy_winit (cross-platform) — no
winit-side debug logging was added for Windows. The bevy_winit logs are sufficient
to verify the fix.

**Linux X11**: Force X11 backend if needed:
```
WAYLAND_DISPLAY= RUST_LOG=warn,bevy_winit::system=debug cargo run --example restore_window --no-default-features
```
The winit fix uses `xconn.available_monitors()`. Same logging situation as Windows.

**Linux Wayland**: No fix needed/possible — Wayland doesn't allow setting absolute
window position. The workaround feature was already a no-op on Wayland.

## Phase 3: Create Upstream PRs (back on macOS)

### Before creating PRs

1. Strip ALL debug logging from both repos:
   - winit: remove debug! calls from `set_outer_position` and `request_inner_size`
     in window_delegate.rs (revert the `use tracing::{debug, ...}` change too)
   - bevy: remove debug! calls from system.rs, revert `use tracing::{debug, ...}`
2. Revert bevy's `Cargo.toml` patch back to the published winit version (the PR
   should not include a patch pointing to a fork)
3. Run `cargo +nightly fmt` in both repos
4. Amend the commits on each branch, force-push to fork

### winit PR

Create first — the bevy PR will reference it.

- Target: `rust-windowing/winit` master (or whatever branch 0.30.x targets)
- Title: `fix: set_outer_position uses target monitor's scale factor`
- Body should explain:
  - The bug: `set_outer_position` converts coordinates using the current window's
    scale factor, not the target monitor's
  - Affects all three platforms (macOS, Windows, X11) but only for cross-type
    conversions (Physical input on macOS, Logical input on Windows/X11)
  - The fix: resolve which monitor contains the target position, use that monitor's
    scale factor for conversion
  - Shared algorithm in `src/monitor.rs` with unit tests
  - Reference issue #4440

### bevy PR

Create second, referencing the winit PR.

- Target: `bevyengine/bevy` main
- Title: `fix: process window position before size in changed_windows`
- Body should explain:
  - This is a no-op with current released winit — position conversion is wrong
    regardless of ordering
  - Once winit ships the `set_outer_position` fix (link to winit PR), ordering
    matters: position must be applied first so the window is on the correct monitor
    (and `scale_factor()` returns the target monitor's value) before `request_inner_size`
    runs
  - The change is safe and backwards-compatible — just moves existing code earlier
    in the function
  - The comment `// Set position before size so the window is on the correct monitor`
    documents the invariant

## Phase 4: Return to Published Dependencies

After PRs are created, revert bevy_window_manager's `Cargo.toml` back to depending
on published crate versions:

```toml
[dependencies]
bevy = "0.18.1"

# Remove the entire [patch.crates-io] section (or restore local paths for
# continued local development of other features)
```

For continued local development of the multi-window feature on the
`refactor/unconditional-types` branch, restore local path patches to the
un-modified bevy/winit checkouts (revert them to their release tags):

```bash
cd ~/rust/winit && git checkout v0.30.13
cd ~/rust/bevy && git checkout v0.18.1
```

Then bevy_window_manager's local path patches work against vanilla releases again,
and the `workaround-winit-4440` feature flag remains active (as it should until
both upstream PRs are released).
