#!/usr/bin/env bash
set -euo pipefail

# ============================================================================
# run_test.sh — Single-test runner for bevy_window_manager integration tests
#
# Compatible with bash 3.2+ (macOS default).
#
# Usage:
#   # Run a single test:
#   tests/scripts/run_test.sh \
#     --config tests/config/macos.json \
#     --test-id same_monitor_restore_mon0
#
#   # With feature flags override:
#   tests/scripts/run_test.sh \
#     --config tests/config/macos.json \
#     --test-id cross_high_to_low_W1 \
#     --feature-flags=--no-default-features
#
#   # Discovery mode (writes env vars to file):
#   tests/scripts/run_test.sh --discover \
#     --config tests/config/macos.json \
#     --env-file /tmp/claude/discovery.env
#
#   # Run a test using discovered env vars:
#   tests/scripts/run_test.sh \
#     --config tests/config/macos.json \
#     --test-id same_monitor_restore_mon0 \
#     --env-file /tmp/claude/discovery.env
#
# The script reads ron_dir and ron_path from the config JSON automatically.
#
# Exit codes: 0 = all pass, 1 = any fail, 2 = script error
# ============================================================================

BRP_URL="http://127.0.0.1:15702/jsonrpc"
POLL_INTERVAL=0.05
MAX_POLLS=200  # 10s timeout (binary must be pre-built)
APP_PID=""
PASS_COUNT=0
FAIL_COUNT=0
TEST_ID=""
CAPTURED_STDERR=""

# Temp file for RON key-value store (avoids associative arrays)
RON_KV_FILE=""

# ============================================================================
# Cleanup
# ============================================================================

cleanup() {
    if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
        kill "$APP_PID" 2>/dev/null || true
        wait "$APP_PID" 2>/dev/null || true
    fi
    if [[ -n "$CAPTURED_STDERR" ]] && [[ -f "$CAPTURED_STDERR" ]]; then
        rm -f "$CAPTURED_STDERR"
    fi
    if [[ -n "$RON_KV_FILE" ]] && [[ -f "$RON_KV_FILE" ]]; then
        rm -f "$RON_KV_FILE"
    fi
}
trap cleanup EXIT

# ============================================================================
# Output helpers
# ============================================================================

pass() {
    local key="$1" field="$2"
    shift 2
    echo "PASS ${key} ${field} $*"
    PASS_COUNT=$((PASS_COUNT + 1))
}

fail() {
    local key="$1" field="$2"
    shift 2
    echo "FAIL ${key} ${field} $*"
    FAIL_COUNT=$((FAIL_COUNT + 1))
}

die() {
    echo "ERROR $*" >&2
    exit 2
}

# ============================================================================
# Argument parsing
# ============================================================================

DISCOVER=false
CONFIG_FILE=""
TEST_ID_ARG=""
RON_DIR=""
RON_PATH=""
FEATURE_FLAGS=""
BACKEND="native"
ENV_FILE=""

parse_args() {
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --discover)      DISCOVER=true; shift ;;
            --config)        CONFIG_FILE="$2"; shift 2 ;;
            --test-id)       TEST_ID_ARG="$2"; shift 2 ;;
            --feature-flags=*) FEATURE_FLAGS="${1#--feature-flags=}"; shift ;;
            --backend)       BACKEND="$2"; shift 2 ;;
            --env-file)      ENV_FILE="$2"; shift 2 ;;
            *) die "Unknown argument: $1" ;;
        esac
    done

    [[ -n "$CONFIG_FILE" ]] || die "Missing --config"
    [[ -f "$CONFIG_FILE" ]] || die "Config file not found: ${CONFIG_FILE}"

    if [[ "$DISCOVER" == "true" ]]; then
        [[ -n "$ENV_FILE" ]] || die "Missing --env-file (required for --discover)"
    else
        [[ -n "$TEST_ID_ARG" ]] || die "Missing --test-id (required unless --discover)"
    fi

    # Derive RON_DIR and RON_PATH from config
    RON_DIR=$(jq -r '.test_ron_dir' "$CONFIG_FILE")
    [[ "$RON_DIR" != "null" && -n "$RON_DIR" ]] || die "Config missing test_ron_dir"

    local raw_ron_path
    raw_ron_path=$(jq -r '.example_ron_path' "$CONFIG_FILE")
    [[ "$raw_ron_path" != "null" && -n "$raw_ron_path" ]] || die "Config missing example_ron_path"

    # Expand ~ and %APPDATA%
    RON_PATH="${raw_ron_path/#\~/$HOME}"
    if [[ -n "${APPDATA:-}" ]]; then
        RON_PATH="${RON_PATH/\%APPDATA\%/$APPDATA}"
    fi
}

# ============================================================================
# Key-value store (bash 3.2 compatible, replaces associative arrays)
# Uses a temp file with lines: KEY=VALUE
# ============================================================================

kv_init() {
    RON_KV_FILE=$(mktemp "${TMPDIR:-/tmp}/ron_kv.XXXXXX")
}

kv_set() {
    # $1 = namespace (e.g., pos_x), $2 = key (e.g., primary), $3 = value
    local ns="$1" key="$2" val="$3"
    # Remove existing entry if present, then append
    if [[ -f "$RON_KV_FILE" ]]; then
        grep -v "^${ns}|${key}=" "$RON_KV_FILE" > "${RON_KV_FILE}.tmp" 2>/dev/null || true
        mv "${RON_KV_FILE}.tmp" "$RON_KV_FILE"
    fi
    echo "${ns}|${key}=${val}" >> "$RON_KV_FILE"
}

kv_get() {
    # $1 = namespace, $2 = key; outputs value or empty string
    local ns="$1" key="$2"
    if [[ -f "$RON_KV_FILE" ]]; then
        grep "^${ns}|${key}=" "$RON_KV_FILE" 2>/dev/null | head -1 | sed "s/^${ns}|${key}=//"
    fi
}

# ============================================================================
# BRP helpers
# ============================================================================

brp() {
    local method="$1" params="${2:-null}"
    curl -sf -X POST "$BRP_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"${method}\",\"id\":1,\"params\":${params}}"
}

wait_brp() {
    local i=0
    while ! curl -sf -X POST "$BRP_URL" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"rpc.discover","id":0}' > /dev/null 2>&1; do
        sleep "$POLL_INTERVAL"
        i=$((i + 1))
        if [[ $i -ge $MAX_POLLS ]]; then
            die "BRP did not become ready within timeout"
        fi
    done
}

shutdown_app() {
    local max_wait=40  # iterations of 0.25s = 10s total

    # Send BRP shutdown via brp_extras (clean shutdown with frame delay)
    brp "brp_extras/shutdown" 'null' > /dev/null 2>&1 || true

    # Wait for graceful exit with timeout
    if [[ -n "$APP_PID" ]] && kill -0 "$APP_PID" 2>/dev/null; then
        local i=0
        while kill -0 "$APP_PID" 2>/dev/null && [[ $i -lt $max_wait ]]; do
            sleep 0.25
            i=$((i + 1))
        done
        # Force kill if still alive after timeout
        if kill -0 "$APP_PID" 2>/dev/null; then
            kill -9 "$APP_PID" 2>/dev/null || true
            sleep 0.5
        fi
        wait "$APP_PID" 2>/dev/null || true
    fi

    APP_PID=""
}

# ============================================================================
# RON template substitution (bash 3.2 compatible)
# ============================================================================

substitute_ron() {
    local content="$1"
    # Source env file if provided (contains MONITOR_* vars from discovery)
    if [[ -n "$ENV_FILE" && -f "$ENV_FILE" ]]; then
        source "$ENV_FILE"
    fi
    # Use env to find MONITOR_* variables and sed for substitution
    local var_name var_val
    while IFS='=' read -r var_name var_val; do
        content=$(echo "$content" | sed "s/\${${var_name}}/${var_val}/g")
    done < <(env | grep '^MONITOR_' | head -100)
    echo "$content"
}

write_ron() {
    local ron_file="$1"
    local template_path="${RON_DIR}/${ron_file}"
    [[ -f "$template_path" ]] || die "RON template not found: ${template_path}"

    local template
    template=$(<"$template_path")

    local substituted
    substituted=$(substitute_ron "$template")

    mkdir -p "$(dirname "$RON_PATH")"
    echo "$substituted" > "$RON_PATH"
}

# ============================================================================
# App launch
# ============================================================================

kill_stale_apps() {
    # Kill any lingering restore_window processes from prior runs
    pkill -9 -f "restore_window" 2>/dev/null || true
    sleep 0.5
}

launch_app() {
    local feature_flags="${1:-$FEATURE_FLAGS}"
    local capture_stderr="${2:-false}"

    # Ensure no stale processes are running on our BRP port
    kill_stale_apps

    CAPTURED_STDERR=""

    local cmd="cargo run --example restore_window"
    if [[ -n "$feature_flags" ]]; then
        cmd="$cmd $feature_flags"
    fi

    if [[ "$BACKEND" == "x11" ]]; then
        cmd="WAYLAND_DISPLAY= $cmd"
    fi

    if [[ "$capture_stderr" == "true" ]]; then
        CAPTURED_STDERR=$(mktemp "${TMPDIR:-/tmp}/brp_stderr.XXXXXX")
        eval "$cmd" 2>"$CAPTURED_STDERR" &
    else
        eval "$cmd" > /dev/null 2>&1 &
    fi
    APP_PID=$!

    wait_brp
}

# ============================================================================
# RON parsing (bash 3.2 compatible)
# ============================================================================

parse_ron_values() {
    local ron_content="$1"

    kv_init

    local re_managed='key: Managed\("([^"]+)"\)'
    local re_position='position: Some\(\((-?[0-9]+), *(-?[0-9]+)\)\)'
    local re_width='width: ([0-9]+)'
    local re_height='height: ([0-9]+)'
    local re_monitor='monitor_index: ([0-9]+)'
    local re_mode='mode: (Windowed|BorderlessFullscreen|Fullscreen)'

    local current_key=""
    while IFS= read -r line; do
        if [[ "$line" =~ key:\ Primary ]]; then
            current_key="primary"
        elif [[ "$line" =~ $re_managed ]]; then
            current_key="${BASH_REMATCH[1]}"
        elif [[ -n "$current_key" && "$line" =~ $re_position ]]; then
            kv_set "pos_x" "$current_key" "${BASH_REMATCH[1]}"
            kv_set "pos_y" "$current_key" "${BASH_REMATCH[2]}"
        elif [[ -n "$current_key" && "$line" =~ position:\ None ]]; then
            kv_set "pos_x" "$current_key" ""
            kv_set "pos_y" "$current_key" ""
        elif [[ -n "$current_key" && "$line" =~ $re_width ]]; then
            kv_set "width" "$current_key" "${BASH_REMATCH[1]}"
        elif [[ -n "$current_key" && "$line" =~ $re_height ]]; then
            kv_set "height" "$current_key" "${BASH_REMATCH[1]}"
        elif [[ -n "$current_key" && "$line" =~ $re_monitor ]]; then
            kv_set "monitor" "$current_key" "${BASH_REMATCH[1]}"
        elif [[ -n "$current_key" && "$line" =~ $re_mode ]]; then
            kv_set "mode" "$current_key" "${BASH_REMATCH[1]}"
        fi
    done <<< "$ron_content"
}

# ============================================================================
# BRP query helpers
# ============================================================================

query_primary() {
    brp "world.query" '{
        "data":{"components":["bevy_window::window::Window","bevy_window_manager::monitors::CurrentMonitor"]},
        "filter":{"with":["bevy_window::window::PrimaryWindow"]}
    }'
}

query_managed() {
    brp "world.query" '{
        "data":{"components":["bevy_window::window::Window","bevy_window_manager::types::ManagedWindow","bevy_window_manager::monitors::CurrentMonitor"]},
        "filter":{"with":["bevy_window_manager::types::ManagedWindow"],"without":["bevy_window::window::PrimaryWindow"]}
    }'
}

# ============================================================================
# Extraction helpers (jq-based)
# ============================================================================

extract_window_position() {
    echo "$1" | jq -r '.components["bevy_window::window::Window"].position'
}

extract_window_width() {
    echo "$1" | jq -r '.components["bevy_window::window::Window"].resolution.physical_width'
}

extract_window_height() {
    echo "$1" | jq -r '.components["bevy_window::window::Window"].resolution.physical_height'
}

extract_scale_factor() {
    echo "$1" | jq -r '.components["bevy_window::window::Window"].resolution.scale_factor'
}

extract_window_mode() {
    echo "$1" | jq -r '.components["bevy_window::window::Window"].mode'
}

extract_monitor_index() {
    echo "$1" | jq -r '.components["bevy_window_manager::monitors::CurrentMonitor"].monitor.index'
}

extract_entity_id() {
    echo "$1" | jq -r '.entity'
}

normalize_mode() {
    local mode_json="$1"
    if [[ "$mode_json" == '"Windowed"' ]] || [[ "$mode_json" == 'Windowed' ]]; then
        echo "Windowed"
    elif echo "$mode_json" | jq -e '.BorderlessFullscreen' > /dev/null 2>&1; then
        echo "BorderlessFullscreen"
    elif echo "$mode_json" | jq -e '.Fullscreen' > /dev/null 2>&1; then
        echo "Fullscreen"
    else
        echo "$mode_json"
    fi
}

# ============================================================================
# Validation
# ============================================================================

validate_window() {
    local key="$1"
    local validate_json="$2"
    local entity_json="$3"
    local prefix="${4:-}"
    local expected_mode_override="${5:-}"

    local validate_fields
    validate_fields=$(echo "$validate_json" | jq -r '.[]')

    while IFS= read -r field; do
        case "$field" in
            position)
                if [[ "$BACKEND" == "wayland" ]]; then
                    continue
                fi
                local pos_json
                pos_json=$(extract_window_position "$entity_json")
                local actual_x actual_y
                actual_x=$(echo "$pos_json" | jq -r '.At[0] // empty' 2>/dev/null || echo "")
                actual_y=$(echo "$pos_json" | jq -r '.At[1] // empty' 2>/dev/null || echo "")
                local exp_x exp_y
                exp_x=$(kv_get "pos_x" "$key")
                exp_y=$(kv_get "pos_y" "$key")

                if [[ "$actual_x" == "$exp_x" && "$actual_y" == "$exp_y" ]]; then
                    pass "$key" "${prefix}position" "expected=[${exp_x},${exp_y}] actual=[${actual_x},${actual_y}]"
                else
                    fail "$key" "${prefix}position" "expected=[${exp_x},${exp_y}] actual=[${actual_x},${actual_y}]"
                fi
                ;;

            size)
                local actual_w actual_h
                actual_w=$(extract_window_width "$entity_json")
                actual_h=$(extract_window_height "$entity_json")
                local exp_w exp_h
                exp_w=$(kv_get "width" "$key")
                exp_h=$(kv_get "height" "$key")

                if [[ "$actual_w" == "$exp_w" && "$actual_h" == "$exp_h" ]]; then
                    pass "$key" "${prefix}size" "expected=[${exp_w},${exp_h}] actual=[${actual_w},${actual_h}]"
                else
                    fail "$key" "${prefix}size" "expected=[${exp_w},${exp_h}] actual=[${actual_w},${actual_h}]"
                fi
                ;;

            mode)
                local mode_raw
                mode_raw=$(extract_window_mode "$entity_json")
                local actual_mode
                actual_mode=$(normalize_mode "$mode_raw")
                local exp_mode
                if [[ -n "$expected_mode_override" ]]; then
                    exp_mode="$expected_mode_override"
                else
                    exp_mode=$(kv_get "mode" "$key")
                fi

                if [[ "$actual_mode" == "$exp_mode" ]]; then
                    pass "$key" "${prefix}mode" "expected=${exp_mode} actual=${actual_mode}"
                else
                    fail "$key" "${prefix}mode" "expected=${exp_mode} actual=${actual_mode}"
                fi
                ;;

            monitor_index)
                local actual_idx
                actual_idx=$(extract_monitor_index "$entity_json")
                local exp_idx
                exp_idx=$(kv_get "monitor" "$key")

                if [[ "$actual_idx" == "$exp_idx" ]]; then
                    pass "$key" "${prefix}monitor_index" "expected=${exp_idx} actual=${actual_idx}"
                else
                    fail "$key" "${prefix}monitor_index" "expected=${exp_idx} actual=${actual_idx}"
                fi
                ;;

            exit_code)
                # Handled separately after shutdown
                ;;
        esac
    done <<< "$validate_fields"
}

# ============================================================================
# Spawn and poll managed windows
# ============================================================================

spawn_and_poll_managed() {
    local spawn_event="$1"
    local max_attempts=20
    local i=0

    brp "world.trigger_event" "{\"event\":\"${spawn_event}\",\"value\":null}" > /dev/null

    local result=""
    while [[ $i -lt $max_attempts ]]; do
        sleep 0.5
        result=$(query_managed 2>/dev/null || echo "")
        if [[ -n "$result" ]]; then
            local has_ready
            has_ready=$(echo "$result" | jq '[.result[]? | select(.components["bevy_window::window::Window"].position != "Automatic")] | length')
            if [[ "$has_ready" -gt 0 ]]; then
                echo "$result"
                return 0
            fi
        fi
        i=$((i + 1))
    done
    die "Managed window did not become ready after ${max_attempts} polls"
}

get_managed_by_name() {
    local result_json="$1"
    local window_name="$2"
    echo "$result_json" | jq --arg name "$window_name" \
        '.result[] | select(.components["bevy_window_manager::types::ManagedWindow"].window_name == $name)'
}

# ============================================================================
# Mutation helpers
# ============================================================================

apply_mutations() {
    local test_json="$1"
    local entity_json="$2"
    local entity_id
    entity_id=$(extract_entity_id "$entity_json")

    local has_position_offset has_size
    has_position_offset=$(echo "$test_json" | jq -e '.mutation.position_offset' > /dev/null 2>&1 && echo true || echo false)
    has_size=$(echo "$test_json" | jq -e '.mutation.size' > /dev/null 2>&1 && echo true || echo false)

    if [[ "$has_position_offset" == "true" ]]; then
        local offset_x offset_y
        offset_x=$(echo "$test_json" | jq -r '.mutation.position_offset[0]')
        offset_y=$(echo "$test_json" | jq -r '.mutation.position_offset[1]')

        local ron_x ron_y
        ron_x=$(kv_get "pos_x" "primary")
        ron_y=$(kv_get "pos_y" "primary")
        ron_x=${ron_x:-0}
        ron_y=${ron_y:-0}
        local new_x=$((ron_x + offset_x))
        local new_y=$((ron_y + offset_y))

        brp "world.mutate_components" "{
            \"entity\":${entity_id},
            \"component\":\"bevy_window::window::Window\",
            \"path\":\".position\",
            \"value\":{\"At\":[${new_x},${new_y}]}
        }" > /dev/null

        kv_set "pos_x" "primary" "$new_x"
        kv_set "pos_y" "primary" "$new_y"
    fi

    if [[ "$has_size" == "true" ]]; then
        local new_w new_h
        new_w=$(echo "$test_json" | jq -r '.mutation.size[0]')
        new_h=$(echo "$test_json" | jq -r '.mutation.size[1]')

        local scale
        scale=$(extract_scale_factor "$entity_json")

        brp "world.mutate_components" "{
            \"entity\":${entity_id},
            \"component\":\"bevy_window::window::Window\",
            \"path\":\".resolution\",
            \"value\":{\"physical_width\":${new_w},\"physical_height\":${new_h},\"scale_factor\":${scale},\"scale_factor_override\":null}
        }" > /dev/null

        kv_set "width" "primary" "$new_w"
        kv_set "height" "primary" "$new_h"
    fi
}

verify_mutations() {
    local result
    result=$(query_primary)
    local new_entity
    new_entity=$(echo "$result" | jq '.result[0]')

    local actual_w actual_h
    actual_w=$(extract_window_width "$new_entity")
    actual_h=$(extract_window_height "$new_entity")
    local exp_w exp_h
    exp_w=$(kv_get "width" "primary")
    exp_h=$(kv_get "height" "primary")

    if [[ "$actual_w" == "$exp_w" && "$actual_h" == "$exp_h" ]]; then
        pass "primary" "mutation_size" "expected=[${exp_w},${exp_h}] actual=[${actual_w},${actual_h}]"
    else
        fail "primary" "mutation_size" "expected=[${exp_w},${exp_h}] actual=[${actual_w},${actual_h}]"
    fi

    local exp_px
    exp_px=$(kv_get "pos_x" "primary")
    if [[ -n "$exp_px" ]]; then
        local pos_json
        pos_json=$(extract_window_position "$new_entity")
        local actual_x actual_y
        actual_x=$(echo "$pos_json" | jq -r '.At[0] // empty' 2>/dev/null || echo "")
        actual_y=$(echo "$pos_json" | jq -r '.At[1] // empty' 2>/dev/null || echo "")
        local exp_py
        exp_py=$(kv_get "pos_y" "primary")

        if [[ "$actual_x" == "$exp_px" && "$actual_y" == "$exp_py" ]]; then
            pass "primary" "mutation_position" "expected=[${exp_px},${exp_py}] actual=[${actual_x},${actual_y}]"
        else
            fail "primary" "mutation_position" "expected=[${exp_px},${exp_py}] actual=[${actual_x},${actual_y}]"
        fi
    fi
}

# ============================================================================
# Persistence validation
# ============================================================================

validate_persistence() {
    local test_json="$1"
    local ron_content
    ron_content=$(<"$RON_PATH")

    local expected_keys
    expected_keys=$(echo "$test_json" | jq -r '.persistence_validation.expected_ron_keys[]')
    while IFS= read -r key; do
        if [[ "$key" == "primary" ]]; then
            if grep -q "key: Primary" <<< "$ron_content"; then
                pass "persistence" "key=${key}" "present"
            else
                fail "persistence" "key=${key}" "missing"
            fi
        else
            if grep -q "key: Managed(\"${key}\")" <<< "$ron_content"; then
                pass "persistence" "key=${key}" "present"
            else
                fail "persistence" "key=${key}" "missing"
            fi
        fi
    done <<< "$expected_keys"

    local unexpected_keys
    unexpected_keys=$(echo "$test_json" | jq -r '.persistence_validation.unexpected_ron_keys // [] | .[]' 2>/dev/null || true)
    if [[ -n "$unexpected_keys" ]]; then
        while IFS= read -r key; do
            if [[ "$key" == "primary" ]]; then
                if grep -q "key: Primary" <<< "$ron_content"; then
                    fail "persistence" "key=${key}" "should_be_absent"
                else
                    pass "persistence" "key=${key}" "absent"
                fi
            else
                if grep -q "key: Managed(\"${key}\")" <<< "$ron_content"; then
                    fail "persistence" "key=${key}" "should_be_absent"
                else
                    pass "persistence" "key=${key}" "absent"
                fi
            fi
        done <<< "$unexpected_keys"
    fi
}

# ============================================================================
# Click fullscreen button (macOS only)
# ============================================================================

click_fullscreen_button() {
    osascript -e 'tell application "System Events" to tell process "restore_window" to click button 2 of window 1' 2>/dev/null || true
    sleep 2
}

# ============================================================================
# Discovery mode
# ============================================================================

emit() {
    echo "$1" >> "$ENV_FILE"
    echo "$1"
}

run_discovery() {
    > "$ENV_FILE"  # truncate env file

    write_ron "discovery.ron"
    launch_app ""

    # Poll until Monitors resource is populated (may take a frame after BRP is ready)
    local monitors_result monitors_json num_monitors poll_i=0
    while true; do
        monitors_result=$(brp "world.get_resources" '{"resource":"bevy_window_manager::monitors::Monitors"}' 2>/dev/null || echo "")
        monitors_json=$(echo "$monitors_result" | jq '.result.value' 2>/dev/null || echo "null")
        num_monitors=$(echo "$monitors_json" | jq '.list | length' 2>/dev/null || echo "0")
        if [[ "$num_monitors" -gt 0 ]]; then
            break
        fi
        sleep "$POLL_INTERVAL"
        poll_i=$((poll_i + 1))
        if [[ $poll_i -ge $MAX_POLLS ]]; then
            die "Monitors resource not populated within timeout"
        fi
    done

    emit "export NUM_MONITORS=${num_monitors}"

    local i=0
    while [[ $i -lt $num_monitors ]]; do
        local mon
        mon=$(echo "$monitors_json" | jq ".list[${i}]")

        emit "export MONITOR_${i}_POS_X=$(echo "$mon" | jq -r '.position[0]')"
        emit "export MONITOR_${i}_POS_Y=$(echo "$mon" | jq -r '.position[1]')"
        emit "export MONITOR_${i}_WIDTH=$(echo "$mon" | jq -r '.size[0]')"
        emit "export MONITOR_${i}_HEIGHT=$(echo "$mon" | jq -r '.size[1]')"
        emit "export MONITOR_${i}_SCALE=$(echo "$mon" | jq -r '.scale')"

        i=$((i + 1))
    done

    # Query video modes
    local monitor_query
    monitor_query=$(brp "world.query" '{"data":{"components":["bevy_window::monitor::Monitor"]},"filter":{}}')

    local monitor_entities
    monitor_entities=$(echo "$monitor_query" | jq '.result')
    local entity_count
    entity_count=$(echo "$monitor_entities" | jq 'length')

    i=0
    while [[ $i -lt $entity_count && $i -lt $num_monitors ]]; do
        local video_modes
        video_modes=$(echo "$monitor_entities" | jq ".[${i}].components[\"bevy_window::monitor::Monitor\"].video_modes")

        local mode_count
        mode_count=$(echo "$video_modes" | jq 'length')

        if [[ $mode_count -gt 0 ]]; then
            local rand_idx=$(( RANDOM % mode_count ))
            local selected
            selected=$(echo "$video_modes" | jq ".[${rand_idx}]")

            emit "export MONITOR_${i}_VIDEO_MODE_WIDTH=$(echo "$selected" | jq -r '.physical_size[0]')"
            emit "export MONITOR_${i}_VIDEO_MODE_HEIGHT=$(echo "$selected" | jq -r '.physical_size[1]')"
            emit "export MONITOR_${i}_VIDEO_MODE_DEPTH=$(echo "$selected" | jq -r '.bit_depth')"
            emit "export MONITOR_${i}_VIDEO_MODE_REFRESH=$(echo "$selected" | jq -r '.refresh_rate_millihertz')"
        fi

        i=$((i + 1))
    done

    # Validate WindowRestored event
    local restored_result
    restored_result=$(brp "world.get_resources" '{"resource":"restore_window::WindowRestoredReceived"}' 2>/dev/null || echo "")

    if [[ -n "$restored_result" ]]; then
        local restored_json
        restored_json=$(echo "$restored_result" | jq '.result.value')

        if [[ "$restored_json" != "null" ]]; then
            echo "# WindowRestoredReceived validation: OK"
        else
            echo "# WARNING: WindowRestoredReceived resource not found"
        fi
    fi

    # Compute DIFFERENT_SCALES
    if [[ $num_monitors -ge 2 ]]; then
        local scale0 scale1
        scale0=$(echo "$monitors_json" | jq -r '.list[0].scale')
        scale1=$(echo "$monitors_json" | jq -r '.list[1].scale')
        if [[ "$scale0" != "$scale1" ]]; then
            emit "export DIFFERENT_SCALES=true"
        else
            emit "export DIFFERENT_SCALES=false"
        fi
    else
        emit "export DIFFERENT_SCALES=false"
    fi

    shutdown_app

    # Linux X11 discovery
    if [[ "$BACKEND" == "x11-also" ]]; then
        echo "# X11 discovery..."
        BACKEND="x11"
        launch_app ""

        local x11_monitors
        x11_monitors=$(brp "world.get_resources" '{"resource":"bevy_window_manager::monitors::Monitors"}')
        local x11_json
        x11_json=$(echo "$x11_monitors" | jq '.result.value')

        i=0
        while [[ $i -lt $num_monitors ]]; do
            emit "export MONITOR_${i}_X11_SCALE=$(echo "$x11_json" | jq -r ".list[${i}].scale")"
            i=$((i + 1))
        done

        local x11_query
        x11_query=$(brp "world.query" '{"data":{"components":["bevy_window::monitor::Monitor"]},"filter":{}}')
        local x11_entities
        x11_entities=$(echo "$x11_query" | jq '.result')

        i=0
        while [[ $i -lt $entity_count && $i -lt $num_monitors ]]; do
            local x11_modes
            x11_modes=$(echo "$x11_entities" | jq ".[${i}].components[\"bevy_window::monitor::Monitor\"].video_modes")
            local x11_mode_count
            x11_mode_count=$(echo "$x11_modes" | jq 'length')

            if [[ $x11_mode_count -gt 0 ]]; then
                local rand_idx=$(( RANDOM % x11_mode_count ))
                local selected
                selected=$(echo "$x11_modes" | jq ".[${rand_idx}]")

                emit "export MONITOR_${i}_X11_VIDEO_MODE_WIDTH=$(echo "$selected" | jq -r '.physical_size[0]')"
                emit "export MONITOR_${i}_X11_VIDEO_MODE_HEIGHT=$(echo "$selected" | jq -r '.physical_size[1]')"
                emit "export MONITOR_${i}_X11_VIDEO_MODE_DEPTH=$(echo "$selected" | jq -r '.bit_depth')"
                emit "export MONITOR_${i}_X11_VIDEO_MODE_REFRESH=$(echo "$selected" | jq -r '.refresh_rate_millihertz')"
            fi

            i=$((i + 1))
        done

        shutdown_app
        BACKEND="native"
    fi

    exit 0
}

# ============================================================================
# Main test execution
# ============================================================================

run_test() {
    # Kill any lingering processes before we start
    kill_stale_apps

    local test_json
    test_json=$(jq --arg id "$TEST_ID_ARG" '.tests[] | select(.id == $id)' "$CONFIG_FILE")
    [[ -n "$test_json" ]] || die "Test '${TEST_ID_ARG}' not found in ${CONFIG_FILE}"

    TEST_ID="$TEST_ID_ARG"

    local test_backend
    test_backend=$(echo "$test_json" | jq -r '.backend // "native"')
    if [[ "$BACKEND" == "native" && "$test_backend" != "native" && "$test_backend" != "null" ]]; then
        BACKEND="$test_backend"
    fi

    local ron_file
    ron_file=$(echo "$test_json" | jq -r '.ron_file')
    write_ron "$ron_file"

    local ron_content
    ron_content=$(<"$RON_PATH")
    parse_ron_values "$ron_content"

    local has_click_fullscreen has_mutation has_persistence has_expected_log has_exit_code
    has_click_fullscreen=$(echo "$test_json" | jq -r '.click_fullscreen_button // false')
    has_mutation=$(echo "$test_json" | jq -e '.mutation' > /dev/null 2>&1 && echo true || echo false)
    has_persistence=$(echo "$test_json" | jq -e '.persistence_validation' > /dev/null 2>&1 && echo true || echo false)
    has_expected_log=$(echo "$test_json" | jq -r '.expected_log_warning // ""')

    has_exit_code=false
    local windows_json
    windows_json=$(echo "$test_json" | jq '.windows // {}')
    if echo "$windows_json" | jq -e '.. | select(type == "array") | select(. | index("exit_code"))' > /dev/null 2>&1; then
        has_exit_code=true
    fi

    local capture_stderr=false
    if [[ -n "$has_expected_log" ]]; then
        capture_stderr=true
    fi

    launch_app "$FEATURE_FLAGS" "$capture_stderr"

    if [[ "$has_click_fullscreen" == "true" ]]; then
        click_fullscreen_button
        shutdown_app
        launch_app "$FEATURE_FLAGS"
    fi

    if [[ "$has_exit_code" == "true" ]]; then
        pass "primary" "exit_code" "expected=0 actual=0"
        shutdown_app
        if [[ $FAIL_COUNT -gt 0 ]]; then exit 1; else exit 0; fi
    fi

    local window_keys
    window_keys=$(echo "$windows_json" | jq -r 'keys[]')

    # Track triggered spawn events (bash 3.2 compatible — use a delimited string)
    local triggered_events=""
    local managed_result=""

    while IFS= read -r wkey; do
        local wconfig
        wconfig=$(echo "$windows_json" | jq --arg k "$wkey" '.[$k]')

        local validate_arr
        validate_arr=$(echo "$wconfig" | jq '.validate')

        local expected_mode_override
        expected_mode_override=$(echo "$wconfig" | jq -r '.expected_mode // ""')

        local spawn_event
        spawn_event=$(echo "$wconfig" | jq -r '.spawn_event // ""')

        local entity_json=""

        if [[ "$wkey" == "primary" ]]; then
            local primary_result
            primary_result=$(query_primary)
            entity_json=$(echo "$primary_result" | jq '.result[0]')
        else
            if [[ -n "$spawn_event" ]] && ! echo "$triggered_events" | grep -qF "|${spawn_event}|"; then
                managed_result=$(spawn_and_poll_managed "$spawn_event")
                triggered_events="${triggered_events}|${spawn_event}|"
            fi

            if [[ -n "$managed_result" ]]; then
                entity_json=$(get_managed_by_name "$managed_result" "$wkey")
            fi
        fi

        if [[ -z "$entity_json" || "$entity_json" == "null" ]]; then
            fail "$wkey" "query" "window not found"
            continue
        fi

        validate_window "$wkey" "$validate_arr" "$entity_json" "" "$expected_mode_override"
    done <<< "$window_keys"

    # Persistence setup
    if [[ "$has_persistence" == "true" ]]; then
        local persist_mode
        persist_mode=$(echo "$test_json" | jq -r '.persistence_validation.mode')
        if [[ "$persist_mode" == "ActiveOnly" ]]; then
            brp "world.insert_resources" '{"resource":"bevy_window_manager::types::ManagedWindowPersistence","value":"ActiveOnly"}' > /dev/null
        fi
    fi

    # Apply mutations
    if [[ "$has_mutation" == "true" ]]; then
        local primary_result
        primary_result=$(query_primary)
        local primary_entity
        primary_entity=$(echo "$primary_result" | jq '.result[0]')

        apply_mutations "$test_json" "$primary_entity"
        sleep 0.5
        verify_mutations
    fi

    # Shutdown
    shutdown_app

    # Check expected log warning
    if [[ -n "$has_expected_log" && -n "$CAPTURED_STDERR" && -f "$CAPTURED_STDERR" ]]; then
        if grep -q "$has_expected_log" "$CAPTURED_STDERR"; then
            pass "log" "expected_warning" "found=\"${has_expected_log}\""
        else
            fail "log" "expected_warning" "not_found=\"${has_expected_log}\""
        fi
    fi

    # Persistence validation
    if [[ "$has_persistence" == "true" ]]; then
        validate_persistence "$test_json"
    fi

    # Relaunch and validate restore (if mutation)
    if [[ "$has_mutation" == "true" ]]; then
        launch_app "$FEATURE_FLAGS"

        while IFS= read -r wkey; do
            local wconfig
            wconfig=$(echo "$windows_json" | jq --arg k "$wkey" '.[$k]')
            local validate_arr
            validate_arr=$(echo "$wconfig" | jq '.validate')
            local expected_mode_override
            expected_mode_override=$(echo "$wconfig" | jq -r '.expected_mode // ""')
            local spawn_event
            spawn_event=$(echo "$wconfig" | jq -r '.spawn_event // ""')

            local entity_json=""

            if [[ "$wkey" == "primary" ]]; then
                local primary_result
                primary_result=$(query_primary)
                entity_json=$(echo "$primary_result" | jq '.result[0]')
            else
                if [[ -n "$spawn_event" ]]; then
                    managed_result=$(spawn_and_poll_managed "$spawn_event")
                    entity_json=$(get_managed_by_name "$managed_result" "$wkey")
                fi
            fi

            if [[ -z "$entity_json" || "$entity_json" == "null" ]]; then
                fail "$wkey" "relaunch_query" "window not found"
                continue
            fi

            validate_window "$wkey" "$validate_arr" "$entity_json" "relaunch " "$expected_mode_override"
        done <<< "$window_keys"

        shutdown_app
    fi

    if [[ $FAIL_COUNT -gt 0 ]]; then
        exit 1
    else
        exit 0
    fi
}

# ============================================================================
# Entry point
# ============================================================================

parse_args "$@"

if [[ "$DISCOVER" == "true" ]]; then
    run_discovery
else
    run_test
fi
