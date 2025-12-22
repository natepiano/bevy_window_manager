# Integration Test Runner

Run automated integration tests for bevy_window_manager using BRP.

## Issue Index

Tests reference these tracked issues via `workaround_keys` in the JSON configs:

| Key | Issue | Platform | Feature Flag | Description |
|-----|-------|----------|--------------|-------------|
| W1 | [winit #4440](https://github.com/rust-windowing/winit/issues/4440) | macOS | `workaround-winit-4440` | `set_outer_position` and `request_inner_size` use current monitor's scale factor instead of target monitor's. When restoring to a different-scale monitor, coordinates are converted incorrectly. |
| W2 | [winit #4041](https://github.com/rust-windowing/winit/issues/4041) | Windows | `workaround-winit-4341` | DPI change causes window bounce/resize when dragging between mixed-DPI monitors. Fix in [winit #4341](https://github.com/rust-windowing/winit/pull/4341). |
| W3 | [winit #3124](https://github.com/rust-windowing/winit/issues/3124) | Windows | `workaround-winit-3124` | Exclusive fullscreen crashes on startup with DX12 due to DXGI flip model limitations. We defer fullscreen until after surface creation via `FullscreenRestoreState`. |
| W4 | [winit #4441](https://github.com/rust-windowing/winit/issues/4441) | macOS | `workaround-winit-4441` | Window size resets to default when dragging between monitors with different scale factors after programmatic resize. AppKit tracks "intended size" per scale factor; programmatic `setContentSize` doesn't update this tracking, but manual resize does. |
| W5 | [winit #4443](https://github.com/rust-windowing/winit/issues/4443) | Linux X11 | `workaround-winit-4443` | On X11, keyboard snap/tile (Meta+Arrow) emits `SurfaceResized` but not `Moved` even when position changed. We query `outer_position()` directly when saving state. Related: [bevy #17576](https://github.com/bevyengine/bevy/issues/17576). |
| W6 | [winit #4445](https://github.com/rust-windowing/winit/issues/4445) | Linux X11 | `workaround-winit-4445` | On X11, `outer_position()` returns a value offset by the title bar height from what was set via `set_outer_position()`. Combined with W5, this causes position drift on each save/restore cycle. We compensate by querying `_NET_FRAME_EXTENTS`. |

**Key prefixes:** W = winit issue

**Usage**: `/test <platform> [monitor]`

**Examples**:
- `/test macos` - Run macOS tests on ALL monitors (auto-moves Zed)
- `/test linux` - Run Linux tests on ALL monitors (auto-moves terminal)
- `/test windows` - Run Windows tests on ALL monitors (auto-moves Zed)

**Arguments**: $ARGUMENTS

**macOS Requirements**:
- Tests run on all monitors automatically - no monitor argument needed
- Zed window is auto-moved between monitors using AppleScript
- Scripts query NSScreen directly for monitor geometry (no temp file dependency)

**Windows Requirements**:
- Tests run on all monitors automatically - no monitor argument needed
- Zed window is auto-moved between monitors using PowerShell Win32 APIs
- Scripts use EnumDisplayMonitors and SetWindowPos for monitor/window manipulation

**Linux Requirements**:
- Must run from XWayland Konsole (launched via `.claude/scripts/linux_test.sh`)
- The script spawns Konsole in XWayland mode so xdotool can detect/move it
- Tests run on all monitors automatically - no monitor argument needed

<CriticalRules>
**STOP AND CONSULT USER IF:**
- Any test fails for any reason
- You encounter unexpected errors or exceptions
- You get confused about what the test expects
- The test results don't make sense
- You're unsure how to proceed

Do NOT continue running more tests after a failure. Stop, explain what happened, and ask the user how to proceed.
</CriticalRules>

<ExecutionSteps>
1. <ParseArguments/>
2. <LinuxEnvironmentCheck/>
3. <LoadTestConfig/>
4. <DiscoverMonitors/>
5. <RunTests/>
6. <FormatResults/>
</ExecutionSteps>

<ParseArguments>
Parse $ARGUMENTS → extract platform.

Valid platforms: macos, windows, linux

Store as ${PLATFORM}. ${MONITOR_INDEX} = "all" for all platforms.
</ParseArguments>

<LinuxEnvironmentCheck>
**Linux only**: Check if running from XWayland Konsole.

1. Run `.claude/scripts/linux_detect_konsole_monitor.sh`

2. **If SUCCESS** (returns "0" or "1"):
   - We're in XWayland Konsole → proceed to <LoadTestConfig/>

3. **If FAILURE** ("No XWayland Konsole found"):
   - Launch `.claude/scripts/linux_test.sh` (in background)
   - Display message: "Launched XWayland Konsole. Run `/test linux` in the new terminal."
   - STOP execution (do not continue to LoadTestConfig)
</LinuxEnvironmentCheck>

<LoadTestConfig>
**Windows**:
Load single unified config: `.claude/config/windows.json`
- Tests are ordered by launch_monitor (Monitor 0 first, then Monitor 1, then human tests)
- Each test has a `launch_monitor` field specifying which monitor Zed must be on

**macOS**:
Load single unified config: `.claude/config/macos.json`
- Tests are ordered by launch_monitor (Monitor 0 first, then Monitor 1, then human tests)
- Each test has a `launch_monitor` field specifying which monitor Zed must be on

**Linux**:
Load single unified config: `.claude/config/linux.json`
- Tests are ordered by backend (Wayland first, then X11) and by launch_monitor within each backend
- Each test has a `launch_monitor` field specifying which monitor the terminal must be on

Extract: platform, example_ron_path, test_ron_dir, tests array.
</LoadTestConfig>

<DiscoverMonitors>
1. Launch with `mcp__brp__brp_launch_bevy_example` target_name "restore_window"

2. Query `mcp__brp__world_get_resources` resource "bevy_window_manager::monitors::Monitors"
   - Store ${MONITORS} array with: index, scale, position, size

3. Query for video modes using `mcp__brp__world_query`:
   - data: `{"components": ["bevy_window::monitor::Monitor"]}`
   - filter: `{}`
   - For each monitor, extract the `video_modes` array
   - Randomly select one video mode per monitor
   - Store all discovered values as <TemplateVariables/>

   **Linux X11 values**: Monitor scales and video modes differ between Wayland and X11/XWayland.
   - Shutdown the Wayland app
   - Relaunch with `WAYLAND_DISPLAY= cargo run --example restore_window` (background)
   - Wait for BRP ready, then:
     - Query Monitors resource again → store as `${MONITOR_X_X11_SCALE}` variables
     - Query Monitor component again → store X11-specific video modes as `${MONITOR_X_X11_VIDEO_MODE_*}` variables
   - Shutdown the X11 app

4. Detect editor/terminal monitor:
   - **macOS**: Run `.claude/scripts/macos_detect_zed_monitor.sh`
     - Outputs "0" or "1" for the monitor index
   - **Windows**: Use <WindowsZedMove/> detect script (same parameters, outputs "0" or "1")
   - **Linux**: Run `.claude/scripts/linux_detect_konsole_monitor.sh`
     - Outputs "0" or "1" for the monitor index
     - If error: STOP with the error message (likely "Must run from XWayland Konsole")

5. Shutdown with `mcp__brp__brp_shutdown`

6. No verification needed for any platform (we auto-move to each monitor).

Compute: ${NUM_MONITORS}, ${DIFFERENT_SCALES}
</DiscoverMonitors>

<MacOSZedMove>
**macOS only**: Move Zed to target monitor before running that monitor's tests.

Run: `.claude/scripts/macos_move_zed_to_monitor.sh <monitor_index>`

The script:
- Positions Zed in left half of target monitor
- Accounts for menu bar
- Sizes window to half width and most of monitor height
- Verifies position with detect script after move
</MacOSZedMove>

<LinuxTerminalMove>
**Linux only**: Move Konsole to target monitor before running that monitor's tests.

Run: `.claude/scripts/linux_move_konsole_to_monitor.sh <monitor_index>`

The script:
- Positions Konsole in upper-left of target monitor
- Accounts for title bar so entire window is on target monitor
- Sizes window to half width and most of monitor height
- Verifies position with detect script after move
</LinuxTerminalMove>

<WindowsZedMove>
**Windows only**: Move/detect Zed on target monitor.

**Move script**:
```
powershell -Command "& '.claude/scripts/windows_move_zed_to_monitor.ps1' -TargetIndex <monitor_index> -Mon0X <mon0_x> -Mon0Y <mon0_y> -Mon0Scale <mon0_scale> -Mon1X (<mon1_x>) -Mon1Y (<mon1_y>) -Mon1Scale <mon1_scale>"
```

**Detect script**:
```
powershell -Command "& '.claude/scripts/windows_detect_zed_monitor.ps1' -Mon0X <mon0_x> -Mon0Y <mon0_y> -Mon0Scale <mon0_scale> -Mon1X (<mon1_x>) -Mon1Y (<mon1_y>) -Mon1Scale <mon1_scale>"
```

**IMPORTANT**: Negative coordinates must be wrapped in parentheses (e.g., `(-1631)`) to prevent PowerShell from interpreting them as parameter names.

**Parameters**: Pass Bevy's physical monitor positions and scale factors from the `Monitors` resource.

Both scripts:
- Match Bevy monitors to Windows monitors by comparing positions (accounting for scale)
- Use Win32 APIs (EnumDisplayMonitors) for monitor enumeration

Move script additionally:
- Positions Zed in left half of the matched Windows monitor
- Accounts for taskbar via work area calculation
- Verifies position with detect script after move
</WindowsZedMove>

<TemplateVariables>
Monitor properties (X = monitor index):
- `${MONITOR_X_POS_X}` → Monitor X position X coordinate
- `${MONITOR_X_POS_Y}` → Monitor X position Y coordinate
- `${MONITOR_X_WIDTH}` → Monitor X width
- `${MONITOR_X_HEIGHT}` → Monitor X height
- `${MONITOR_X_SCALE}` → Monitor X scale factor

Video mode properties (randomly selected per monitor):
- `${MONITOR_X_VIDEO_MODE_WIDTH}` → Video mode width
- `${MONITOR_X_VIDEO_MODE_HEIGHT}` → Video mode height
- `${MONITOR_X_VIDEO_MODE_DEPTH}` → Video mode bit depth
- `${MONITOR_X_VIDEO_MODE_REFRESH}` → Video mode refresh rate (millihertz)

Linux X11-specific values (differ from Wayland):
- `${MONITOR_X_X11_SCALE}` → X11 scale factor (may differ from Wayland)
- `${MONITOR_X_X11_VIDEO_MODE_WIDTH}` → X11 video mode width
- `${MONITOR_X_X11_VIDEO_MODE_HEIGHT}` → X11 video mode height
- `${MONITOR_X_X11_VIDEO_MODE_DEPTH}` → X11 video mode bit depth
- `${MONITOR_X_X11_VIDEO_MODE_REFRESH}` → X11 video mode refresh rate (millihertz)
</TemplateVariables>

<RunTests>
**Windows**: Run tests in JSON order. Automated tests are grouped by launch_monitor, human tests are at the end.

**Phase 1: Automated Tests**
1. Move Zed to Monitor 0 using <WindowsZedMove/>
2. Run all automated tests with `launch_monitor: 0`
3. Move Zed to Monitor 1 using <WindowsZedMove/>
4. Run all automated tests with `launch_monitor: 1`

**Phase 2: Human-Assisted Tests**
5. For each human test: move Zed to the test's `launch_monitor`, then run the test

**IMPORTANT**: Human tests appear at the END of the JSON array. Run them in order, moving Zed to each test's `launch_monitor` before running.

---

**macOS**: Run tests in JSON order. Automated tests are grouped by launch_monitor, human tests are at the end.

**Phase 1: Automated Tests**
1. Move Zed to Monitor 0 using <MacOSZedMove/>
2. Run all automated tests with `launch_monitor: 0`
3. Move Zed to Monitor 1 using <MacOSZedMove/>
4. Run all automated tests with `launch_monitor: 1`

**Phase 2: Human-Assisted Tests**
5. For each human test: move Zed to the test's `launch_monitor`, then run the test

**IMPORTANT**: Human tests appear at the END of the JSON array. Run them in order, moving Zed to each test's `launch_monitor` before running.

**Linux**: Run tests in JSON order. Automated tests are grouped by backend then launch_monitor, human tests are at the end.

**Phase 1: Wayland Automated Tests**
1. Move Konsole to Monitor 0 using <LinuxTerminalMove/>
2. Run all Wayland automated tests with `launch_monitor: 0`
3. Move Konsole to Monitor 1 using <LinuxTerminalMove/>
4. Run all Wayland automated tests with `launch_monitor: 1`

**Phase 2: X11 Automated Tests**
5. Move Konsole to Monitor 0 using <LinuxTerminalMove/>
6. Run all X11 automated tests with `launch_monitor: 0`
7. Move Konsole to Monitor 1 using <LinuxTerminalMove/>
8. Run all X11 automated tests with `launch_monitor: 1`

**Phase 3: Human-Assisted Tests**
9. For each human test: move Konsole to the test's `launch_monitor`, then run the test

**IMPORTANT**: Human tests appear at the END of the JSON array. Run them in order, moving Konsole to each test's `launch_monitor` before running.

For each test in order:

1. **Check requirements** - skip if not met

2. **Resolve target_monitor**:
   - `"launch"` → current monitor (the one Konsole is on)
   - `"other"` → first monitor that isn't current
   - Explicit number → that monitor index

3. **Execute test** using <TestSequence/>

4. **Record result**
</RunTests>

<TestSequence>
Unified test sequence that adapts based on JSON fields.

## Step 1: Determine Test Type

Check which fields exist:
- Is `automation: "human_only"` or `"human_assisted"`? → Execute <HumanTestFlow/> instead of Steps 2-10
- Has `click_fullscreen_button`? → This is a green button test (macOS only)
- Has `workaround_validation`? → This is a workaround test (run twice)
- Has `ron_file` + `mutation`? → This is a mutation test
- Has `ron_file` only? → This is a simple restore test

## Step 2: Write RON File

1. Read the RON template from `${test_ron_dir}/${test.ron_file}`
2. Substitute <TemplateVariables/> with discovered monitor values
   - **Linux X11 tests**: For tests with `backend: "x11"`, substitute `${MONITOR_X_VIDEO_MODE_*}`
     with the X11-specific values (`${MONITOR_X_X11_VIDEO_MODE_*}`) instead
3. Write the substituted content to `${example_ron_path}`:
   - **Windows**: `${example_ron_path}` = `%APPDATA%\restore_window\windows.ron` → `C:\Users\<user>\AppData\Roaming\restore_window\windows.ron`
   - If the file already exists, Read it first (Write tool requires reading existing files before overwriting)
   - If the file doesn't exist, just Write it directly

Use Read tool then Write tool (NEVER heredoc).

**Windows PowerShell notes**:
- Use `-not` for negation (bash exclamation mark syntax doesn't work in PowerShell)
- Example: `if (-not (Test-Path ...))` is correct

## Step 3: Launch App

**Linux backend handling** (if platform is Linux and test has `backend` field):

For `backend: "x11"`:
- Prepend `WAYLAND_DISPLAY= ` to force X11/XWayland mode
- Command: `WAYLAND_DISPLAY= cargo run --example restore_window ${FEATURE_FLAGS}`

For `backend: "wayland"`:
- Use standard launch (no env modification)

**If test has `expected_log_warning`**:
- Must launch with Bash to capture logs (not mcp__brp__brp_launch_bevy_example)
- Use Bash with `run_in_background: true`:
  ```
  RUST_LOG=bevy_window_manager=warn cargo run --example restore_window
  ```
  **CRITICAL**: The command must be EXACTLY this. Do NOT add:
  - No `2>&1` redirects
  - No `&` backgrounding (use `run_in_background: true` parameter instead)
  - No `sleep` commands
  - No `echo` commands
  - No pipes or additional shell commands
- Wait for BRP ready: poll `mcp__brp__brp_status` with app_name "restore_window" until status is "running_with_brp"
- After shutdown, use `TaskOutput` to retrieve logs and check for expected warning string

**If test has `workaround_validation`**:
- Determine feature flags from `workaround_validation.build_without` or default
- Use Bash with `run_in_background: true`:
  ```
  cargo run --example restore_window ${FEATURE_FLAGS}
  ```
  **CRITICAL**: Do NOT pipe output (no `| head`, `| tail`, `2>&1 | grep`, etc.)
  The command must be EXACTLY: `cargo run --example restore_window ${FEATURE_FLAGS}`
- Wait for BRP ready: poll `mcp__brp__brp_status` with app_name "restore_window" until status is "running_with_brp"

**Otherwise** (no workaround_validation, no expected_log_warning):
- Use `mcp__brp__brp_launch_bevy_example` with target_name "restore_window"

## Step 3.5: Click Fullscreen Button (if `click_fullscreen_button: true`)

**macOS only**: This step triggers macOS native fullscreen via the green button.

1. Click the fullscreen button via AppleScript:
   ```
   osascript -e 'tell application "System Events" to tell process "restore_window" to click button 2 of window 1'
   ```
   (Button 2 is the AXFullScreenButton - the green maximize/fullscreen button)

2. Wait 2 seconds for macOS fullscreen animation to complete

3. Shutdown the app with `mcp__brp__brp_shutdown`

4. Relaunch with `mcp__brp__brp_launch_bevy_example`

5. Proceed to Step 4 to validate that fullscreen mode was restored

## Step 4: Validate Restore

**If `validate` contains `"exit_code"`** (e.g., fullscreen panic tests):
- Skip window query - validation happens after shutdown in Step 7
- Proceed directly to Step 7 (Shutdown)
- After shutdown, check exit code:
  - Exit code 0 = PASS (clean exit)
  - Exit code 134 (SIGABRT) = FAIL (panic)
  - Other non-zero = FAIL with details

**Otherwise** (normal window validation):

Run exactly these queries based on `validate` array contents:

### For `"position"` and/or `"size"` and/or `"mode"` validation:

Query Window: `mcp__brp__world_query`
- data: `{"components": ["bevy_window::window::Window"]}`
- filter: `{"with": ["bevy_window::window::PrimaryWindow"]}`

Fields to check:
- `"position"`: `window.position` matches `{"At": [POS_X, POS_Y]}`
  - **Note**: On Wayland (`backend: "wayland"`), position is always `Automatic` - skip position validation
  - **Note**: On X11 (`backend: "x11"`), Window.position reflects the client area position (below title bar),
    not the frame position, due to winit bug #4445. After a mutation, Window.position will immediately show
    the client area position (mutation value + frame_top). The RON saves this client area position.
- `"size"`: `window.resolution.physical_width` and `window.resolution.physical_height` match expected
- `"mode"`: `window.mode` matches expected
  - **If test has `expected_mode``: verify `window.mode` matches `expected_mode` instead of RON file
    (used when actual mode differs from requested, e.g., exclusive→borderless fallback)

### For `"monitor_index"` validation:

Query CurrentMonitor: `mcp__brp__world_query`
- data: `{"components": ["bevy_window_manager::monitors::CurrentMonitor"]}`
- filter: `{"with": ["bevy_window::window::PrimaryWindow"]}`

Fields to check:
- `current_monitor.monitor_index` matches expected target monitor index

**IMPORTANT**: Do NOT use `Window.resolution.scale_factor` for monitor validation.
On Windows, `scale_factor` does not update when windows are programmatically moved between monitors.
Always use `CurrentMonitor.monitor_index` as the source of truth.

**If test has `expected_log_warning`**:
- Check captured log output contains the expected warning string
- PASS if warning found, FAIL if not

Record validation result (PASS or FAIL with details).

## Step 5: Apply Mutations (if `mutation` field exists)

For position: mutate `.position` → `{"At": [X, Y]}`

For size:
1. Query current scale_factor first
2. Mutate `.resolution` → `{"physical_width": W, "physical_height": H, "scale_factor": CURRENT, "scale_factor_override": null}`

For mode: mutate `.mode` with appropriate format:
- Windowed: `"Windowed"`
- BorderlessFullscreen: `{"BorderlessFullscreen": {"Index": N}}`
- Fullscreen: query Monitor for video_modes, then `{"Fullscreen": [{"Index": N}, {"Specific": {...}}]}`

## Step 6: Verify Mutations (if mutations were applied)

Query Window again and validate new values match mutation targets.

## Step 7: Shutdown

**If test has `quit_method: "osascript_cmd_q"`**:
- Use osascript to send real Cmd+Q:
  ```
  osascript -e 'tell application "System Events" to keystroke "q" using command down'
  ```
- Wait for background task to complete using `TaskOutput` with the task_id from Step 3
- Store the exit code for validation

**Otherwise**:
- Use `mcp__brp__brp_shutdown` with app_name "restore_window"

## Step 8: Validate RON Saved (if mutations were applied)

Read RON file, verify saved values match mutation values.

## Step 9: Relaunch and Validate Restore (if mutations were applied)

Launch again (same method as Step 3), query Window, verify restore matches saved state.

Shutdown after validation.

## Step 10: Handle Workaround Validation

**If test has `workaround_validation`**:

The test runs TWICE - once without workaround, once with.

**X11 Position Workaround Tests (workaround-winit-4445)**:

For X11 position tests, the workaround ensures position STABILITY across save/restore cycles.
Due to winit bug #4445, `outer_position()` returns client area position, not frame position.
Without the workaround, position drifts by frame_top (title bar height) on each restore.

Validation approach for X11 position workaround tests:
1. After mutation, record Window.position (this is the client area position)
2. Shutdown (position saves to RON as client area position)
3. Relaunch and query Window.position again
4. **Phase 1 (without workaround)**: Window.position will DRIFT from saved position (FAIL = bug confirmed ✓)
5. **Phase 2 (with workaround)**: Window.position will MATCH saved position (PASS = workaround works ✓)

**Phase 1: WITHOUT workaround**
- Feature flags: `workaround_validation.build_without`
- Run Steps 2-7 (skip 8-9 for exit_code tests)
- Expected: Test should FAIL (bug manifests)
  - For X11 position validation: Window.position after restore drifts from saved RON position
  - For exit_code validation: exit code != 0 (panic)
- If PASS: WARNING "Bug not reproduced"
- If FAIL: "Bug confirmed ✓"

**Phase 2: WITH workaround**
- Feature flags: (none - use default features)
- Run Steps 2-7 (skip 8-9 for exit_code tests)
- Expected: Test should PASS (workaround fixes bug)
  - For X11 position validation: Window.position after restore matches saved RON position (stable)
  - For exit_code validation: exit code == 0 (clean exit)
- If PASS: "Workaround verified ✓"
- If FAIL: FAIL "Workaround did not fix bug"

**Final result**:
- PASS: Bug confirmed in Phase 1 AND fixed in Phase 2
- PARTIAL: Bug not reproduced but workaround works
- FAIL: Workaround did not fix the bug

<HumanTestFlow>
1. **CRITICAL - Move editor to test's launch_monitor FIRST**: Read the test's `launch_monitor` field and move the editor there:
   - macOS: `.claude/scripts/macos_move_zed_to_monitor.sh <launch_monitor>`
   - Windows: Use <WindowsZedMove/> with target = `<launch_monitor>`
   - Linux: `.claude/scripts/linux_move_konsole_to_monitor.sh <launch_monitor>`
2. Write RON from `${test_ron_dir}/${test.ron_file}` to `${example_ron_path}`
3. **If has `workaround_validation`**: run Phase 1 first (build_without flags), then Phase 2 (default features)
4. Launch app using Bash with `run_in_background: true`
5. Display instructions to user:
   - For workaround tests: use `instructions_without_workaround` (Phase 1) or `instructions_with_workaround` (Phase 2)
   - For regular tests: use `instructions` array
6. Use AskUserQuestion: "Did the test pass?" with options based on `success_criteria`
7. Shutdown app, record result
8. For workaround tests: repeat steps 4-7 for Phase 2
</HumanTestFlow>
</TestSequence>

<FormatResults>
```
## Test Results: ${PLATFORM}

| Test | Monitor | Status | Details |
|------|---------|--------|---------|
| ${test.id} | ${test.launch_monitor} | ${STATUS} | ${DETAILS} |

**Summary**: ${PASSED} passed, ${FAILED} failed, ${SKIPPED} skipped
```

Status icons: ✓ PASS, ✗ FAIL, ⊘ SKIP
</FormatResults>
