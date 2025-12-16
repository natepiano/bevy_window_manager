# Integration Test Runner

Run automated integration tests for bevy_window_manager using BRP.

**Usage**: `/test <platform> <monitor>`

**Examples**:
- `/test macos 0` - Run macOS tests on monitor 0
- `/test windows 1` - Run Windows tests on monitor 1
- `/test linux 0` - Run Linux tests (Wayland + X11) on monitor 0

**Arguments**: $ARGUMENTS

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
2. <LoadTestConfig/>
3. <DiscoverMonitors/>
4. <RunTests/>
5. <FormatResults/>
</ExecutionSteps>

<ParseArguments>
Parse $ARGUMENTS → extract platform and monitor_index.

Valid platforms: macos, windows, linux

Store as ${PLATFORM} and ${MONITOR_INDEX}.
</ParseArguments>

<LoadTestConfig>
Load `.claude/config/${PLATFORM}_monitor${MONITOR_INDEX}.json`

Extract: platform, launch_monitor, example_ron_path, test_ron_dir, tests array.

Open the RON file in Zed for inspection: `zed ${example_ron_path}`
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

   **Linux X11 video modes**: Video modes differ between Wayland and X11/XWayland.
   - Shutdown the Wayland app
   - Relaunch with `WAYLAND_DISPLAY= cargo run --example restore_window` (background)
   - Wait for BRP ready, then query Monitor component again
   - Store X11-specific video modes as `${MONITOR_X_X11_VIDEO_MODE_*}` variables
   - Shutdown the X11 app

4. Detect terminal monitor:
   - **macOS**: `osascript -e 'tell application "System Events" to get position of first window of process "Zed"'`
     - Find which monitor contains this position → ${DETECTED_MONITOR}
   - **Linux**: Skip detection (single monitor 0 for now)

5. Shutdown with `mcp__brp__brp_shutdown`

6. If ${DETECTED_MONITOR} != ${MONITOR_INDEX}: STOP with error
   - **Linux**: Skip this check (no detection available)

Compute: ${NUM_MONITORS}, ${DIFFERENT_SCALES}
</DiscoverMonitors>

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

Linux X11-specific video modes (differ from Wayland):
- `${MONITOR_X_X11_VIDEO_MODE_WIDTH}` → X11 video mode width
- `${MONITOR_X_X11_VIDEO_MODE_HEIGHT}` → X11 video mode height
- `${MONITOR_X_X11_VIDEO_MODE_DEPTH}` → X11 video mode bit depth
- `${MONITOR_X_X11_VIDEO_MODE_REFRESH}` → X11 video mode refresh rate (millihertz)
</TemplateVariables>

<RunTests>
For each test in order:

1. **Check requirements** - skip if not met

2. **Resolve target_monitor**:
   - `"launch"` → monitor at ${MONITOR_INDEX}
   - `"other"` → first monitor that isn't ${MONITOR_INDEX}

3. **Execute test** using <TestSequence/>

4. **Record result**

Human tests (`automation: "human_only"`) run last, one at a time with user prompts.
</RunTests>

<TestSequence>
Unified test sequence that adapts based on JSON fields.

## Step 1: Determine Test Type

Check which fields exist:
- Is `automation: "human_only"`? → Execute <HumanTestFlow/> instead of Steps 2-10
- Has `workaround_validation`? → This is a workaround test (run twice)
- Has `ron_file` + `mutation`? → This is a mutation test
- Has `ron_file` only? → This is a simple restore test

## Step 2: Write RON File

1. Read the RON template from `${test_ron_dir}/${test.ron_file}`
2. Substitute <TemplateVariables/> with discovered monitor values
   - **Linux X11 tests**: For tests with `backend: "x11"`, substitute `${MONITOR_X_VIDEO_MODE_*}`
     with the X11-specific values (`${MONITOR_X_X11_VIDEO_MODE_*}`) instead
3. Write the substituted content to `${example_ron_path}`

Use Read tool then Write tool (NEVER heredoc).

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

## Step 4: Validate Restore

**If `validate` contains `"exit_code"`** (e.g., fullscreen panic tests):
- Skip window query - validation happens after shutdown in Step 7
- Proceed directly to Step 7 (Shutdown)
- After shutdown, check exit code:
  - Exit code 0 = PASS (clean exit)
  - Exit code 134 (SIGABRT) = FAIL (panic)
  - Other non-zero = FAIL with details

**Otherwise** (normal window validation):
Query Window: `mcp__brp__world_query`
- data: `{"components": ["bevy_window::window::Window"]}`
- filter: `{"with": ["bevy_window::window::PrimaryWindow"]}`

Check fields in `validate` array:
- `"position"`: window.position matches {"At": [POS_X, POS_Y]}
  - **Note**: On Wayland (`backend: "wayland"`), position is always `Automatic` - skip position validation
- `"size"`: window.resolution.physical_width/height match expected
- `"monitor_index"`: scale_factor matches target monitor's scale
- `"mode"`: window.mode matches expected
  - **If test has `expected_mode`**: verify window.mode matches `expected_mode` instead of RON file
    (used when actual mode differs from requested, e.g., exclusive→borderless fallback)

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

**Phase 1: WITHOUT workaround**
- Feature flags: `workaround_validation.build_without`
- Run Steps 2-7 (skip 8-9 for exit_code tests)
- Expected: Test should FAIL (bug manifests)
  - For window validation: values don't match expected
  - For exit_code validation: exit code != 0 (panic)
- If PASS: WARNING "Bug not reproduced"
- If FAIL: "Bug confirmed ✓"

**Phase 2: WITH workaround**
- Feature flags: (none - use default features)
- Run Steps 2-7 (skip 8-9 for exit_code tests)
- Expected: Test should PASS (workaround fixes bug)
  - For window validation: values match expected
  - For exit_code validation: exit code == 0 (clean exit)
- If PASS: "Workaround verified ✓"
- If FAIL: FAIL "Workaround did not fix bug"

**Final result**:
- PASS: Bug confirmed in Phase 1 AND fixed in Phase 2
- PARTIAL: Bug not reproduced but workaround works
- FAIL: Workaround did not fix the bug

<HumanTestFlow>
1. Write RON from `${test_ron_dir}/${test.ron_file}` to `${example_ron_path}`
2. **If has `workaround_validation`**: run Phase 1 first (build_without flags), then Phase 2 (default features)
3. Launch app using Bash with `run_in_background: true`
4. Display instructions to user:
   - For workaround tests: use `instructions_without_workaround` (Phase 1) or `instructions_with_workaround` (Phase 2)
   - For regular tests: use `instructions` array
5. Use AskUserQuestion: "Did the test pass?" with options based on `success_criteria`
6. Shutdown app, record result
7. For workaround tests: repeat steps 3-6 for Phase 2
</HumanTestFlow>
</TestSequence>

<FormatResults>
```
## Test Results: ${PLATFORM} monitor ${MONITOR_INDEX}

| Test | Status | Details |
|------|--------|---------|
| ${test.id} | ${STATUS} | ${DETAILS} |

**Summary**: ${PASSED} passed, ${FAILED} failed, ${SKIPPED} skipped
```

Status icons: ✓ PASS, ✗ FAIL, ⊘ SKIP
</FormatResults>
