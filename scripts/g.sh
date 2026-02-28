#!/usr/bin/env bash
# g.sh — commit, push current branch, build jot, optional release
set -Eeuo pipefail
trap 'echo "✖︎ Error on line $LINENO"; exit 1' ERR

# Usage:
#   ./g.sh [-m "commit msg"] [--release]
# Env:
#   GIT_PROFILE=onedusk   # if you use `git s-p` alias, we'll call it if configured

msg="chore: update"
do_release=0

while [[ $# -gt 0 ]]; do
  case "$1" in
    -m|--message) msg="${2:?}"; shift 2 ;;
    --release)    do_release=1; shift ;;
    *) echo "Unknown arg: $1"; exit 2 ;;
  esac
done

# Optional: switch git profile if alias exists and var provided
: "${GIT_PROFILE:=onedusk}"
if git config --get alias.s-p >/dev/null 2>&1; then
  git s-p "$GIT_PROFILE"
fi

# Ensure we're inside a git repo
git rev-parse --is-inside-work-tree >/dev/null

# Stage & commit only if there are changes
if ! git diff --quiet || ! git diff --cached --quiet; then
  git add -A
  git commit -m "$msg"
fi

# Push current branch
branch="$(git rev-parse --abbrev-ref HEAD)"
git push -u origin "$branch"

# --- Build / Release ---
if [[ $do_release -eq 1 ]]; then
  make release
  exit 0
fi

# Default: just build for the current platform
make build
