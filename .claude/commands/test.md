# Integration Test Runner v3

Run automated integration tests for bevy_window_manager using `tests/scripts/run_test.py`.

## Issue Index

Tests reference these tracked issues via `workaround_keys` in the JSON configs:

| Key | Issue | Platform | Feature Flag | Description |
|-----|-------|----------|--------------|-------------|
| W1 | [winit #4440](https://github.com/rust-windowing/winit/issues/4440) | macOS | `workaround-winit-4440` | `set_outer_position` and `request_inner_size` use current monitor's scale factor instead of target monitor's. |
| W2 | [winit #4041](https://github.com/rust-windowing/winit/issues/4041) | Windows | `workaround-winit-4341` | DPI change causes window bounce/resize when dragging between mixed-DPI monitors. |
| W3 | [winit #3124](https://github.com/rust-windowing/winit/issues/3124) | Windows | `workaround-winit-3124` | Exclusive fullscreen crashes on startup with DX12. |
| W5 | [winit #4443](https://github.com/rust-windowing/winit/issues/4443) | Linux X11 | `workaround-winit-4443` | Keyboard snap/tile doesn't emit `Moved` event on X11. |
| W6 | [winit #4445](https://github.com/rust-windowing/winit/issues/4445) | Linux X11 | `workaround-winit-4445` | `outer_position()` returns offset by title bar height on X11. |

**Usage**: `/test [flags]`

**Examples**:
- `/test` - Auto-detect OS and run all tests
- `/test single-monitor` - Force single-monitor mode

**Arguments**: $ARGUMENTS

<CriticalRules>
**STOP AND CONSULT USER IF:**
- Any test fails for any reason
- You encounter unexpected errors or exceptions
- The test results don't make sense

Do NOT continue running more tests after a failure. Stop, explain what happened, and ask the user how to proceed.
</CriticalRules>

<ExecutionSteps>
1. <PreBuild/>
2. <LinuxEnvironmentCheck/>
3. <LoadTestConfig/>
4. <DiscoverMonitors/>
5. <WindowsMonitorValidation/> (Windows only)
6. <RunTests/>
7. <FormatResults/>
</ExecutionSteps>

<PreBuild>
Run the prebuild script via Python. It auto-detects platform, creates `/tmp/claude`, and compiles both binary variants.

Parse optional user flags first:
- `single-monitor` → forced_single_monitor=true

```bash
python3 tests/scripts/run_test.py --prebuild
```

Parse output lines:
- `PLATFORM=macos|linux|windows` — store as ${PLATFORM}
- `CONFIG=tests/config/<platform>.json` — store as ${config_file}
- `BUILD_DEFAULT=ok|failed`
- `BUILD_VARIANT_N=...` — each unique workaround feature flag combination
- `BUILD_VARIANTS=N ok|skipped`

If any build reports `failed`, STOP and report the error.
</PreBuild>

<LinuxEnvironmentCheck>
**Linux only**: Check if running from XWayland Konsole.

1. Run `tests/scripts/linux_detect_konsole_monitor.sh`
2. **If SUCCESS**: proceed
3. **If FAILURE**: Launch `tests/scripts/linux_test.sh [single-monitor]` and STOP
</LinuxEnvironmentCheck>

<LoadTestConfig>
Read the platform-specific config file (already determined in PreBuild):

Extract: platform, example_ron_path, test_ron_dir, tests array.

**Expand `example_ron_path`**: Replace `~` with `$HOME`, `%APPDATA%` with the actual value.
</LoadTestConfig>

<DiscoverMonitors>
**IMPORTANT**: Use `dangerouslyDisableSandbox: true` for ALL commands in this section (discovery script, editor/terminal detection, and monitor move scripts all require GPU/Metal/AppleScript access).

Run discovery via the script:

```bash
python3 tests/scripts/run_test.py --discover \
  --config "${config_file}" \
  --backend "${discover_backend}" \
  --env-file /tmp/claude/discovery.env
```

**discover_backend**:
- macOS/Windows: `native`
- Linux: `x11-also` (discovers both Wayland and X11 scales/video modes)

The script writes env vars to `/tmp/claude/discovery.env` and prints them to stdout.
Parse the stdout output to extract:
- `NUM_MONITORS`, `DIFFERENT_SCALES`
- `MONITOR_X_POS_X`, `MONITOR_X_POS_Y`, `MONITOR_X_WIDTH`, `MONITOR_X_HEIGHT`, `MONITOR_X_SCALE`
- `MONITOR_X_VIDEO_MODE_WIDTH`, `MONITOR_X_VIDEO_MODE_HEIGHT`, `MONITOR_X_VIDEO_MODE_DEPTH`, `MONITOR_X_VIDEO_MODE_REFRESH`
- Linux X11: `MONITOR_X_X11_SCALE`, `MONITOR_X_X11_VIDEO_MODE_*`

**After discovery**:

1. Detect editor/terminal monitor:
   - **macOS**: Run `tests/scripts/macos_detect_zed_monitor.sh`
   - **Windows**: Run `powershell -Command "& 'tests/scripts/windows_detect_zed_monitor.ps1' ..."`
   - **Linux**: Run `tests/scripts/linux_detect_konsole_monitor.sh`

2. Compute:
   - `SINGLE_MONITOR_MODE` = true if `NUM_MONITORS == 1` OR `forced_single_monitor == true`

3. If single-monitor mode: display skip count
</DiscoverMonitors>

<WindowsMonitorValidation>
**Windows only**. If `NUM_MONITORS >= 2`, check `MONITOR_0_SCALE > MONITOR_1_SCALE`.

If false, STOP and display the monitor layout mismatch message (same as test.md).
</WindowsMonitorValidation>

<SingleMonitorFiltering>
Skip tests requiring multiple monitors (same rules as test.md):
1. `requires.min_monitors: 2`
2. `launch_monitor: 1`
3. RON targets monitor 1 (`_to_mon1`, `_mon1` suffix, `monitor_index: 1`)
4. Cross-monitor test (ID contains `cross`, `requires.different_scales: true`)
</SingleMonitorFiltering>

<TemplateVariables>
Monitor properties (X = monitor index):
- `${MONITOR_X_POS_X}`, `${MONITOR_X_POS_Y}`, `${MONITOR_X_WIDTH}`, `${MONITOR_X_HEIGHT}`, `${MONITOR_X_SCALE}`
- `${MONITOR_X_VIDEO_MODE_WIDTH}`, `${MONITOR_X_VIDEO_MODE_HEIGHT}`, `${MONITOR_X_VIDEO_MODE_DEPTH}`, `${MONITOR_X_VIDEO_MODE_REFRESH}`

Linux X11:
- `${MONITOR_X_X11_SCALE}`, `${MONITOR_X_X11_VIDEO_MODE_*}`
</TemplateVariables>

<MacOSZedMove>
Run `tests/scripts/macos_move_zed_to_monitor.sh <monitor_index>` with `dangerouslyDisableSandbox: true` (AppleScript access needed).
</MacOSZedMove>

<LinuxTerminalMove>
Run `tests/scripts/linux_move_konsole_to_monitor.sh <monitor_index>` with `dangerouslyDisableSandbox: true`.
</LinuxTerminalMove>

<WindowsZedMove>
Use the PowerShell move/detect scripts with Bevy monitor parameters (same as test.md). Use `dangerouslyDisableSandbox: true`.
</WindowsZedMove>

<RunTests>
## Pre-flight: Apply Single-Monitor Filtering

If `SINGLE_MONITOR_MODE` is true, filter using <SingleMonitorFiltering/> rules.

---

**Test execution order** — group by `launch_monitor`, human tests last:

**macOS/Windows**: Move editor to monitor 0 → run all `launch_monitor: 0` tests → move editor to monitor 1 → run all `launch_monitor: 1` tests → human tests.
**Linux**: Move terminal to monitor 0 → Wayland mon0 tests → move terminal to monitor 1 → Wayland mon1 tests → move terminal to monitor 0 → X11 mon0 tests → move terminal to monitor 1 → X11 mon1 tests → human tests.

**CRITICAL**: The Bevy app launches on whichever monitor has focus (where the editor/terminal is). Before running a group of tests, you MUST move the editor/terminal TO the group's `launch_monitor` using the platform-specific move script (<MacOSZedMove/>, <LinuxTerminalMove/>, or <WindowsZedMove/>). If you skip this step, tests that depend on launch monitor (especially cross-monitor and workaround validation tests) will produce incorrect results.

---

For each automated test:

1. **Check requirements** — skip if not met
2. **Check automation type** — if `human_only` or `human_assisted`, defer to <HumanTestFlow/>

3. **If `workaround_validation`** — run two phases:

   **Phase 1 (WITHOUT workaround)**:
   ```bash
   python3 tests/scripts/run_test.py \
     --config "${config_file}" \
     --test-id "${test_id}" \
     --feature-flags=${workaround_validation.build_without} \
     --backend "${backend}" \
     --env-file /tmp/claude/discovery.env
   ```
   Expected: FAIL (bug manifests). If PASS: WARNING "Bug not reproduced".

   **Phase 2 (WITH workaround)**:
   ```bash
   python3 tests/scripts/run_test.py \
     --config "${config_file}" \
     --test-id "${test_id}" \
     --feature-flags=${workaround_validation.build_with} \
     --backend "${backend}" \
     --env-file /tmp/claude/discovery.env
   ```
   Expected: PASS (workaround fixes bug). If FAIL: FAIL "Workaround did not fix bug".

   **Final result**:
   - PASS: Phase 1 FAIL + Phase 2 PASS → "Bug confirmed, workaround verified"
   - PARTIAL: Phase 1 PASS + Phase 2 PASS → "Bug not reproduced but workaround works"
   - FAIL: Phase 2 FAIL → "Workaround did not fix bug"

4. **Otherwise (normal test)** — single run:
   ```bash
   python3 tests/scripts/run_test.py \
     --config "${config_file}" \
     --test-id "${test_id}" \
     --backend "${backend}" \
     --env-file /tmp/claude/discovery.env
   ```
   Capture stdout. Parse PASS/FAIL lines. Exit code 0 = all pass, 1 = any fail.

5. **Record result** from script output.

**IMPORTANT**: Always pass `--env-file /tmp/claude/discovery.env` to all script invocations. The script reads this file for MONITOR_* template substitution.

**IMPORTANT**: Use `dangerouslyDisableSandbox: true` for all `run_test.py` invocations that launch the Bevy app (discovery + test runs — GPU/Metal access needed by cargo run).

**IMPORTANT for Linux X11 tests**: For tests with `backend: "x11"`, the script handles the `WAYLAND_DISPLAY=` prefix internally. For X11 video mode overrides, modify `/tmp/claude/discovery.env` before calling the script: replace `MONITOR_X_VIDEO_MODE_*` values with their `MONITOR_X_X11_VIDEO_MODE_*` counterparts, then restore after the test.
</RunTests>

<HumanTestFlow>
Human tests use `run_test.py --human-setup` which writes the RON, launches the app **without test mode** (keyboard/controls enabled for human interaction), waits for restore, prints instructions, then exits (leaving the app running).

**IMPORTANT**: When manually relaunching via BRP MCP for human verification (e.g., to check if state was restored), do NOT pass env vars that disable controls. Specifically:
- Do NOT set `BWM_TEST_MODE` — the human needs keyboard input
- For X11 backend tests, still pass `WAYLAND_DISPLAY: ""` to ensure X11 mode

1. Move editor/terminal to test's `launch_monitor`

2. **If `workaround_validation`** — run two phases:

   **Phase 1 (WITHOUT workaround)**:
   ```bash
   python3 tests/scripts/run_test.py --human-setup \
     --config "${config_file}" \
     --test-id "${test_id}" \
     --feature-flags=${workaround_validation.build_without} \
     --backend "${backend}" \
     --env-file /tmp/claude/discovery.env
   ```
   Parse output: `INSTRUCTIONS_START`...`INSTRUCTIONS_END` for instructions, `CRITERIA_*` for success criteria.
   Display instructions to user.
   Use AskUserQuestion with the criteria.
   Shutdown app via BRP MCP tool, record result.

   **Phase 2 (WITH workaround)**:
   ```bash
   python3 tests/scripts/run_test.py --human-setup \
     --config "${config_file}" \
     --test-id "${test_id}" \
     --feature-flags=${workaround_validation.build_with} \
     --backend "${backend}" \
     --env-file /tmp/claude/discovery.env
   ```
   Same flow: parse output, display instructions, ask user, shutdown, record.

3. **Otherwise (non-workaround human test)** — single run:
   ```bash
   python3 tests/scripts/run_test.py --human-setup \
     --config "${config_file}" \
     --test-id "${test_id}" \
     --backend "${backend}" \
     --env-file /tmp/claude/discovery.env
   ```
   Parse output, display instructions, ask user, shutdown app via BRP MCP tool, record result.

**Output format** from `--human-setup`:
- `HUMAN_TEST_READY` — app is running and ready
- `TEST_ID=...` — test identifier
- `DESCRIPTION=...` — test description
- `INSTRUCTIONS_START` / `INSTRUCTIONS_END` — block of instruction lines
- `CRITERIA_PASS=...` / `CRITERIA_FAIL=...` — pass/fail criteria (dict style)
- `CRITERIA=...` — single criteria string (string style)
</HumanTestFlow>

<FormatResults>
```
## Test Results: ${PLATFORM}

| Test | Monitor | Status | Details |
|------|---------|--------|---------|
| ${test.id} | ${test.launch_monitor} | ${STATUS} | ${DETAILS} |

**Summary**: ${PASSED} passed, ${FAILED} failed, ${SKIPPED} skipped
```

Status icons: ✓ PASS, ✗ FAIL, ⊘ SKIP, ~ PARTIAL

**Details** column: For script-run tests, show the PASS/FAIL line count. For failures, show the first FAIL line's details.
</FormatResults>
