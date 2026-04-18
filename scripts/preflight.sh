#!/usr/bin/env bash

set -euo pipefail
shopt -s nullglob

MAX_HANDOFF_LINES=20

FILE_RULE_LABEL='*.rs,*.toml,*.yaml,*.yml,*.md,*.json,*.sh,*.py,*.js,*.ts,*.tsx,*.jsx,*.zsh,*.fish,*.nu'

resolve_root() {
  local dir=$PWD

  while [[ $dir != / ]]; do
    if [[ -e "$dir/.git" ]]; then
      printf '%s\n' "$dir"
      return
    fi
    dir=$(dirname "$dir")
  done

  printf '%s\n' "$PWD"
}

resolve_git_dir() {
  local root=$1
  local git_path=$root/.git

  if [[ -d $git_path ]]; then
    printf '%s\n' "$git_path"
    return
  fi

  if [[ -f $git_path ]]; then
    local git_ref
    git_ref=$(sed -n 's/^gitdir: //p' "$git_path")
    if [[ -n $git_ref ]]; then
      if [[ $git_ref = /* ]]; then
        printf '%s\n' "$git_ref"
      else
        printf '%s\n' "$root/$git_ref"
      fi
      return
    fi
  fi

  printf '\n'
}

resolve_branch() {
  local git_dir=$1
  local head_file=$git_dir/HEAD

  if [[ ! -f $head_file ]]; then
    printf 'unknown\n'
    return
  fi

  local head
  head=$(<"$head_file")

  if [[ $head == ref:\ refs/heads/* ]]; then
    printf '%s\n' "${head#ref: refs/heads/}"
    return
  fi

  printf '%s\n' "${head:0:7}"
}

is_relevant_file() {
  case "$1" in
    *.rs|*.toml|*.yaml|*.yml|*.md|*.json|*.sh|*.py|*.js|*.ts|*.tsx|*.jsx|*.zsh|*.fish|*.nu)
      return 0
      ;;
    *)
      return 1
      ;;
  esac
}

print_relevant_inputs() {
  local path

  for path in "$@"; do
    if is_relevant_file "$path"; then
      printf '  %s\n' "$path"
    fi
  done
}

print_handoffs() {
  local root=$1
  local found=0
  local path
  local candidates=(
    "$root"/.ctx/HANDOFF*.yaml
    "$root"/.ctx/HANDOFF*.yml
  )

  for path in "${candidates[@]}"; do
    [[ -f $path ]] || continue
    found=1
    printf '\n  %s\n' "$path"
    sed -n "1,${MAX_HANDOFF_LINES}p" "$path" | sed 's/^/  /'
  done

  if [[ $found -eq 0 ]]; then
    printf '  (none found)\n'
  fi
}

ROOT=$(resolve_root)
GIT_DIR=$(resolve_git_dir "$ROOT")
BRANCH=$(resolve_branch "$GIT_DIR")

printf 'preflight | %s | %s\n' "$BRANCH" "$ROOT"
printf 'file rule: %s\n' "$FILE_RULE_LABEL"

if [[ $# -gt 0 ]]; then
  printf '\nrelevant inputs\n'
  print_relevant_inputs "$@"
fi

printf '\nhandoff yaml\n'
print_handoffs "$ROOT"
