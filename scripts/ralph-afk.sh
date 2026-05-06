#!/usr/bin/env bash
set -euo pipefail

VERBOSE="${RALPH_VERBOSE:-0}"
ARGS=()
for arg in "$@"; do
  case "$arg" in
    -v|--verbose)
      VERBOSE=1
      ;;
    *)
      ARGS+=("$arg")
      ;;
  esac
done
set -- "${ARGS[@]}"

ROOT_DIR="$(git rev-parse --show-toplevel)"
RUNNER="${RALPH_RUNNER:-${1:-codex}}"
MAX_ITERATIONS="${2:-10}"
PROMPT_FILE="$ROOT_DIR/scripts/ralph-prompt.md"

BOLD='\033[1m'
GREEN='\033[0;32m'
YELLOW='\033[0;33m'
CYAN='\033[0;36m'
RED='\033[0;31m'
DIM='\033[2m'
RESET='\033[0m'

info()    { echo -e "${CYAN}${BOLD}$*${RESET}"; }
success() { echo -e "${GREEN}${BOLD}$*${RESET}"; }
warn()    { echo -e "${YELLOW}${BOLD}$*${RESET}"; }
error()   { echo -e "${RED}${BOLD}$*${RESET}" >&2; }
dim()     { echo -e "${DIM}$*${RESET}"; }

case "$RUNNER" in
  codex | claude) ;;
  *)
    error "error: unsupported runner: $RUNNER"
    error "usage: $0 [-v|--verbose] [codex|claude] [max-iterations]"
    error "   or: RALPH_RUNNER=codex|claude $0 [max-iterations]"
    exit 1
    ;;
esac

command -v "$RUNNER" >/dev/null 2>&1 || { error "error: $RUNNER not found on PATH"; exit 1; }
[ -f "$PROMPT_FILE" ] || { error "error: prompt file not found: $PROMPT_FILE"; exit 1; }

# Auto-detect feature slug from issues/ dirs with open issues
detect_feature_slug() {
  [ -n "${RALPH_FEATURE_SLUG:-}" ] && { echo "$RALPH_FEATURE_SLUG"; return; }

  local candidates=() dir slug issue st

  for dir in "$ROOT_DIR/issues"/*/; do
    [ -d "$dir" ] || continue
    slug=$(basename "$dir")
    shopt -s nullglob
    for issue in "$dir"[0-9][0-9][0-9]-*.md "$dir"issues/[0-9][0-9][0-9]-*.md; do
      st=$(awk '/^status:/ { print $2; exit }' "$issue" 2>/dev/null || true)
      case "$st" in done|complete) continue ;; esac
      candidates+=("$slug")
      break
    done
    shopt -u nullglob
  done

  case "${#candidates[@]}" in
    0) error "error: no feature with open issues found under issues/; set RALPH_FEATURE_SLUG"; exit 1 ;;
    1) echo "${candidates[0]}" ;;
    *) error "error: multiple features with open issues found — set RALPH_FEATURE_SLUG:"
       for slug in "${candidates[@]}"; do error "  issues/$slug"; done
       exit 1 ;;
  esac
}

FEATURE_SLUG=$(detect_feature_slug)
RALPH_DIR="$ROOT_DIR/issues/$FEATURE_SLUG"
PROGRESS_FILE="$RALPH_DIR/progress.txt"
HOOK_PATH="$ROOT_DIR/.git/hooks/pre-push"
BACKUP_HOOK_PATH="$ROOT_DIR/.git/hooks/pre-push.ralph-backup"

mkdir -p "$RALPH_DIR"
[ -f "$PROGRESS_FILE" ] || printf "# Ralph Progress\n\nFeature: %s\nStarted: %s\n\n" "$FEATURE_SLUG" "$(date -Is)" > "$PROGRESS_FILE"

restore_hook() {
  if [ -f "$BACKUP_HOOK_PATH" ]; then
    mv "$BACKUP_HOOK_PATH" "$HOOK_PATH"
  elif [ -f "$HOOK_PATH" ] && grep -q "ralph-afk no-push guard" "$HOOK_PATH"; then
    rm -f "$HOOK_PATH"
  fi
}

install_hook() {
  [ -f "$HOOK_PATH" ] && [ ! -f "$BACKUP_HOOK_PATH" ] && mv "$HOOK_PATH" "$BACKUP_HOOK_PATH"
  printf '#!/usr/bin/env bash\necho "ralph-afk no-push guard: this workflow may commit but must never push." >&2\nexit 1\n' > "$HOOK_PATH"
  chmod +x "$HOOK_PATH"
}

issues() { shopt -s nullglob; echo "$RALPH_DIR"/[0-9][0-9][0-9]-*.md "$RALPH_DIR"/issues/[0-9][0-9][0-9]-*.md; shopt -u nullglob; }
issue_status() { awk '/^status:/ { print $2; exit }' "$1" 2>/dev/null || echo "unknown"; }
is_done() { local s; s=$(issue_status "$1"); [[ "$s" == "done" || "$s" == "complete" ]]; }

all_done() {
  local issue
  for issue in $(issues); do is_done "$issue" || return 1; done
  return 0
}

open_issue_count() {
  local issue n=0
  for issue in $(issues); do
    is_done "$issue" || n=$((n + 1))
  done
  echo "$n"
}

snapshot_statuses() {
  local issue
  for issue in $(issues); do echo "$(basename "$issue" .md)=$(issue_status "$issue")"; done
}

print_open_issues() {
  local issue name st
  for issue in $(issues); do
    is_done "$issue" && continue
    name=$(basename "$issue" .md)
    st=$(issue_status "$issue")
    dim "  · $name ($st)"
  done
}

print_newly_completed() {
  local before="$1" issue name st_before n=0
  for issue in $(issues); do
    name=$(basename "$issue" .md)
    st_before=$(grep "^$name=" <<<"$before" | cut -d= -f2 || echo "unknown")
    case "$st_before" in done|complete) continue ;; esac
    if is_done "$issue"; then
      success "  ✓ $name"
      n=$((n + 1))
    fi
  done
  if [ "$n" -eq 0 ]; then
    warn "  no issue status changed"
  fi
  return 0
}

run_agent() {
  local env_prefix="RALPH_FEATURE_SLUG=$FEATURE_SLUG RALPH_FEATURE_DIR=$RALPH_DIR RALPH_PROGRESS_FILE=$PROGRESS_FILE"
  case "$RUNNER" in
    codex)
      env RALPH_FEATURE_SLUG="$FEATURE_SLUG" RALPH_FEATURE_DIR="$RALPH_DIR" RALPH_PROGRESS_FILE="$PROGRESS_FILE" \
        codex --ask-for-approval never exec --cd "$ROOT_DIR" --sandbox danger-full-access - < "$PROMPT_FILE"
      ;;
    claude)
      local model_args=()
      [ -n "${RALPH_CLAUDE_MODEL:-}" ] && model_args=(--model "$RALPH_CLAUDE_MODEL")
      env RALPH_FEATURE_SLUG="$FEATURE_SLUG" RALPH_FEATURE_DIR="$RALPH_DIR" RALPH_PROGRESS_FILE="$PROGRESS_FILE" \
        claude -p --verbose \
          --permission-mode "${RALPH_CLAUDE_PERMISSION_MODE:-bypassPermissions}" \
          --add-dir "$ROOT_DIR" \
          "${model_args[@]}" < "$PROMPT_FILE"
      ;;
  esac
}

trap restore_hook EXIT
install_hook

echo
info "Ralph AFK  feature: $FEATURE_SLUG  runner: $RUNNER"
dim "  pending: $(open_issue_count)"

for i in $(seq 1 "$MAX_ITERATIONS"); do
  iter_start=$(date +%s)

  status_before=$(snapshot_statuses)
  _out=$(mktemp)
  set +e
  if [[ "$VERBOSE" == "1" ]]; then
    run_agent 2>&1 | tee "$_out" | sed \
      's|<promise>COMPLETE</promise>|\x1b[1;32m◆ COMPLETE\x1b[0m|g;
       s|<promise>BLOCKED</promise>|\x1b[1;33m◆ BLOCKED\x1b[0m|g'
    agent_status=${PIPESTATUS[0]}
  else
    run_agent >"$_out" 2>&1
    agent_status=$?
  fi
  set -e
  output=$(cat "$_out")

  echo
  print_newly_completed "$status_before"
  elapsed=$(( $(date +%s) - iter_start ))
  dim "  task finished in ${elapsed}s"

  if [[ "$agent_status" -ne 0 ]]; then
    echo
    warn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    warn "  Ralph runner exited with status $agent_status."
    warn "  Last output:"
    tail -80 "$_out" >&2
    warn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    rm -f "$_out"
    exit "$agent_status"
  fi

  if all_done; then
    echo
    success "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    success "  Ralph complete: all local issues done."
    success "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    rm -f "$_out"
    exit 0
  fi

  dim "  remaining: $(open_issue_count)"

  if grep -qx "<promise>BLOCKED</promise>" <<<"$output"; then
    echo
    warn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    warn "  Ralph stopped: all remaining issues are blocked."
    warn "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    rm -f "$_out"
    exit 1
  fi

  rm -f "$_out"
  sleep 2
done

warn "Ralph reached max iterations ($MAX_ITERATIONS) without completion."
exit 1
