#!/usr/bin/env bash
set -euo pipefail

BINARY_NAME="monoize"
PM2_NAME="monoize"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
DEPLOY_DIR="/opt/monoize"
WATCHDOG_TIMEOUT_SECONDS=300

configuration_fail() {
    printf '[FAIL] %s\n' "$1" >&2
    exit 2
}

configure_test_mode() {
    local test_mode="${MONOIZE_DEPLOY_TEST_MODE:-}"
    local test_root_input="${MONOIZE_DEPLOY_TEST_ROOT:-}"
    local test_timeout="${MONOIZE_DEPLOY_TEST_WATCHDOG_TIMEOUT_SECONDS:-}"

    if [ -z "$test_mode" ]; then
        if [ -n "$test_root_input" ] || [ -n "$test_timeout" ]; then
            configuration_fail "Deployment test overrides require MONOIZE_DEPLOY_TEST_MODE=1"
        fi
        return
    fi

    if [ "$test_mode" != "1" ]; then
        configuration_fail "MONOIZE_DEPLOY_TEST_MODE must be exactly 1"
    fi
    if [ -z "$test_root_input" ] || [ -z "$test_timeout" ]; then
        configuration_fail "Deployment test mode requires an explicit test root and watchdog timeout"
    fi

    local test_root
    test_root="$(realpath -e -- "$test_root_input" 2>/dev/null)" \
        || configuration_fail "Deployment test root must be an existing directory"
    local test_root_name
    test_root_name="$(basename -- "$test_root")"
    if [ "$test_root" != "$SCRIPT_DIR" ] \
        || [[ "$test_root_name" != .deploy-watchdog-test.* ]] \
        || [ "$test_root_name" = ".deploy-watchdog-test." ]; then
        configuration_fail "Deployment test root must be the executing script directory named .deploy-watchdog-test.<suffix>"
    fi
    if ! [[ "$test_timeout" =~ ^[0-9]+$ ]] \
        || [ "$test_timeout" -lt 1 ] \
        || [ "$test_timeout" -gt 30 ]; then
        configuration_fail "Deployment test watchdog timeout must be an integer from 1 through 30"
    fi

    DEPLOY_DIR="$test_root/deploy"
    WATCHDOG_TIMEOUT_SECONDS="$test_timeout"
}

configure_test_mode

WATCHDOG_DIR="$DEPLOY_DIR/.deploy-watchdog"
WATCHDOG_ID_FILE="$WATCHDOG_DIR/current_id"
WATCHDOG_PID_FILE="$WATCHDOG_DIR/current_pid"
WATCHDOG_IDENTITY_FILE="$WATCHDOG_DIR/current_identity"
WATCHDOG_META_FILE="$WATCHDOG_DIR/current_backup"

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

step() { echo -e "${GREEN}[DEPLOY]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; exit 1; }

ensure_watchdog_dir() {
    mkdir -p "$WATCHDOG_DIR"
}

clear_watchdog_state() {
    rm -f \
        "$WATCHDOG_ID_FILE" \
        "$WATCHDOG_PID_FILE" \
        "$WATCHDOG_IDENTITY_FILE" \
        "$WATCHDOG_META_FILE"
}

watchdog_id_is_current() {
    local expected_id="$1"
    local current_id=""
    if [ -f "$WATCHDOG_ID_FILE" ]; then
        IFS= read -r current_id < "$WATCHDOG_ID_FILE" || true
    fi
    [ "$current_id" = "$expected_id" ]
}

watchdog_process_group_is_live() {
    local pid="$1"
    [[ "$pid" =~ ^[0-9]+$ ]] \
        && [ "$pid" -gt 1 ] \
        && kill -0 -- "-$pid" 2>/dev/null
}

watchdog_is_session_leader() {
    local pid="$1"
    local session_id=""
    local process_group_id=""
    [[ "$pid" =~ ^[0-9]+$ ]] && [ "$pid" -gt 1 ] || return 1
    read -r session_id process_group_id \
        < <(ps -o sid=,pgid= -p "$pid" 2>/dev/null) || return 1
    [ "$session_id" = "$pid" ] && [ "$process_group_id" = "$pid" ]
}

process_start_time() {
    local pid="$1"
    local stat_line=""
    IFS= read -r stat_line < "/proc/$pid/stat" 2>/dev/null || return 1
    local stat_fields_text="${stat_line##*) }"
    local -a stat_fields=()
    read -r -a stat_fields <<< "$stat_fields_text"
    # Field 22 is index 19 after removing the PID and parenthesized command fields.
    [ "${#stat_fields[@]}" -gt 19 ] || return 1
    printf '%s\n' "${stat_fields[19]}"
}

watchdog_command_matches() {
    local pid="$1"
    local deploy_id="$2"
    local backup_path="$3"
    local -a process_args=()
    mapfile -d '' -t process_args < "/proc/$pid/cmdline" 2>/dev/null || return 1
    [ "${#process_args[@]}" -ge 5 ] \
        && [ "${process_args[1]}" = "$SCRIPT_DIR/deploy.sh" ] \
        && [ "${process_args[2]}" = "__watchdog" ] \
        && [ "${process_args[3]}" = "$deploy_id" ] \
        && [ "${process_args[4]}" = "$backup_path" ]
}

watchdog_identity_matches() {
    local pid="$1"
    local process_identity="$2"
    local deploy_id="$3"
    local backup_path="$4"
    local observed_identity=""
    observed_identity="$(process_start_time "$pid")" || return 1
    watchdog_is_session_leader "$pid" \
        && watchdog_process_group_is_live "$pid" \
        && [ "$observed_identity" = "$process_identity" ] \
        && watchdog_command_matches "$pid" "$deploy_id" "$backup_path"
}

wait_for_watchdog_group_exit() {
    local pid="$1"
    local attempt
    for ((attempt = 0; attempt < 100; attempt++)); do
        if ! watchdog_process_group_is_live "$pid"; then
            return 0
        fi
        sleep 0.02
    done
    ! watchdog_process_group_is_live "$pid"
}

terminate_watchdog_group() {
    local pid="$1"
    kill -TERM -- "-$pid" 2>/dev/null || true
    if wait_for_watchdog_group_exit "$pid"; then
        return 0
    fi
    kill -KILL -- "-$pid" 2>/dev/null || true
    wait_for_watchdog_group_exit "$pid"
}

cancel_watchdog() {
    ensure_watchdog_dir
    local termination_failed=0
    if [ -f "$WATCHDOG_PID_FILE" ] \
        && [ -f "$WATCHDOG_IDENTITY_FILE" ] \
        && [ -f "$WATCHDOG_ID_FILE" ] \
        && [ -f "$WATCHDOG_META_FILE" ]; then
        local pid="" process_identity="" deploy_id="" backup_path=""
        IFS= read -r pid < "$WATCHDOG_PID_FILE" || true
        IFS= read -r process_identity < "$WATCHDOG_IDENTITY_FILE" || true
        IFS= read -r deploy_id < "$WATCHDOG_ID_FILE" || true
        IFS= read -r backup_path < "$WATCHDOG_META_FILE" || true
        if watchdog_identity_matches "$pid" "$process_identity" "$deploy_id" "$backup_path"; then
            terminate_watchdog_group "$pid" || termination_failed=1
        elif [[ "$pid" =~ ^[0-9]+$ ]] && [ "$pid" -gt 1 ]; then
            warn "Ignoring stale rollback watchdog state for unverified PID $pid"
        fi
    fi
    clear_watchdog_state
    if [ "$termination_failed" -ne 0 ]; then
        fail "Rollback watchdog process group did not exit after SIGKILL"
    fi
}

rollback_binary() {
    local backup_path="$1"
    cp "$backup_path" "$DEPLOY_DIR/${BINARY_NAME}.rollback"
    mv "$DEPLOY_DIR/${BINARY_NAME}.rollback" "$DEPLOY_DIR/$BINARY_NAME"
}

restore_backup_after_restart_failure() {
    local backup_path="$1"
    warn "PM2 restart failed; restoring backup binary from $backup_path"
    rollback_binary "$backup_path"
    if ! pm2 restart "$PM2_NAME"; then
        fail "PM2 restart failed after restoring backup binary"
    fi
    pm2 save || warn "PM2 save failed after restoring backup (non-fatal)"
    fail "PM2 restart failed for new binary; restored previous binary"
}

arm_watchdog() {
    local backup_path="$1"
    ensure_watchdog_dir
    local deploy_id
    deploy_id="$(date +%Y%m%d%H%M%S)-$$"
    printf '%s\n' "$deploy_id" > "$WATCHDOG_ID_FILE"
    printf '%s\n' "$backup_path" > "$WATCHDOG_META_FILE"

    local nohup_bin
    local setsid_bin
    nohup_bin="$(command -v nohup)" || fail "nohup is required to arm the rollback watchdog"
    setsid_bin="$(command -v setsid)" || fail "setsid is required to arm the rollback watchdog"

    "$nohup_bin" "$setsid_bin" "$BASH" "$SCRIPT_DIR/deploy.sh" \
        __watchdog "$deploy_id" "$backup_path" </dev/null >/dev/null 2>&1 &
    local launcher_pid="$!"
    disown "$launcher_pid" 2>/dev/null || true

    local attempt
    local watchdog_pid=""
    local watchdog_identity=""
    for ((attempt = 0; attempt < 100; attempt++)); do
        if [ -f "$WATCHDOG_PID_FILE" ] && [ -f "$WATCHDOG_IDENTITY_FILE" ]; then
            IFS= read -r watchdog_pid < "$WATCHDOG_PID_FILE" || true
            IFS= read -r watchdog_identity < "$WATCHDOG_IDENTITY_FILE" || true
            if watchdog_identity_matches \
                "$watchdog_pid" \
                "$watchdog_identity" \
                "$deploy_id" \
                "$backup_path"; then
                step "Rollback watchdog armed for ${WATCHDOG_TIMEOUT_SECONDS}s. Run ./deploy.sh cancel-watchdog to keep the new binary."
                return
            fi
        fi
        if ! watchdog_id_is_current "$deploy_id"; then
            break
        fi
        sleep 0.05
    done

    cancel_watchdog
    fail "Rollback watchdog failed to start in an independent session"
}

run_watchdog() {
    local deploy_id="$1"
    local backup_path="$2"
    local watchdog_pid="$BASHPID"
    local pid_temp="$WATCHDOG_PID_FILE.$watchdog_pid"
    local identity_temp="$WATCHDOG_IDENTITY_FILE.$watchdog_pid"
    local process_identity=""

    if ! watchdog_is_session_leader "$watchdog_pid" || ! watchdog_id_is_current "$deploy_id"; then
        exit 1
    fi
    process_identity="$(process_start_time "$watchdog_pid")" || exit 1

    printf '%s\n' "$watchdog_pid" > "$pid_temp"
    printf '%s\n' "$process_identity" > "$identity_temp"
    if ! watchdog_id_is_current "$deploy_id"; then
        rm -f "$pid_temp" "$identity_temp"
        exit 0
    fi
    mv "$identity_temp" "$WATCHDOG_IDENTITY_FILE"
    mv "$pid_temp" "$WATCHDOG_PID_FILE"
    if ! watchdog_id_is_current "$deploy_id"; then
        rm -f "$WATCHDOG_PID_FILE" "$WATCHDOG_IDENTITY_FILE"
        exit 0
    fi

    sleep "$WATCHDOG_TIMEOUT_SECONDS"

    if ! watchdog_id_is_current "$deploy_id"; then
        exit 0
    fi

    if [ ! -f "$backup_path" ]; then
        clear_watchdog_state
        exit 0
    fi

    rollback_binary "$backup_path"
    pm2 restart "$PM2_NAME"
    pm2 save || true
    if watchdog_id_is_current "$deploy_id"; then
        clear_watchdog_state
    fi
}

case "${1:-deploy}" in
    __watchdog)
        if [ "$#" -ne 3 ]; then
            configuration_fail "Invalid internal watchdog invocation"
        fi
        run_watchdog "$2" "$3"
        exit 0
        ;;
esac

case "${1:-deploy}" in
    cancel-watchdog)
        cancel_watchdog
        step "Rollback watchdog cancelled."
        exit 0
        ;;
    deploy)
        ;;
    *)
        fail "Unknown subcommand: ${1}. Supported: deploy, cancel-watchdog"
        ;;
esac

ensure_watchdog_dir
cancel_watchdog

BACKUP=""

# --- 1. Frontend build ---
step "Building frontend..."
(cd "$SCRIPT_DIR/frontend" && bun run build) || fail "Frontend build failed"

# --- 2. Release build ---
step "Building release binary..."
(cd "$SCRIPT_DIR" && cargo build --release) || fail "Cargo release build failed"

# --- 3. Backup current binary ---
if [ -f "$DEPLOY_DIR/$BINARY_NAME" ]; then
    BACKUP="$DEPLOY_DIR/${BINARY_NAME}.bak.$(date +%Y%m%d%H%M%S)"
    step "Backing up current binary to $BACKUP"
    cp "$DEPLOY_DIR/$BINARY_NAME" "$BACKUP"
    # Keep only the 3 most recent backups
    ls -t "$DEPLOY_DIR"/${BINARY_NAME}.bak.* 2>/dev/null | tail -n +4 | xargs -r rm -f
fi

# --- 4. Atomic swap + restart ---
step "Deploying binary to $DEPLOY_DIR..."
cp "$SCRIPT_DIR/target/release/$BINARY_NAME" "$DEPLOY_DIR/${BINARY_NAME}.next"
mv "$DEPLOY_DIR/${BINARY_NAME}.next" "$DEPLOY_DIR/$BINARY_NAME"

step "Restarting PM2 process..."
if ! pm2 restart "$PM2_NAME"; then
    if [ -n "$BACKUP" ]; then
        restore_backup_after_restart_failure "$BACKUP"
    fi
    fail "PM2 restart failed"
fi
pm2 save || warn "PM2 save failed (non-fatal)"

if [ -n "$BACKUP" ]; then
    arm_watchdog "$BACKUP"
else
    warn "No previous binary found; rollback watchdog not armed."
fi

step "Deploy complete."
