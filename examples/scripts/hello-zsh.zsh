#!/usr/bin/env zsh

name="world"

while (( $# > 0 )); do
  case "$1" in
    --name)
      if (( $# > 1 )); then
        name="$2"
        shift
      fi
      ;;
  esac
  shift
done

print -r -- "hello from zsh, $name!"
