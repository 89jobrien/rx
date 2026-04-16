#!/usr/bin/env bash

name="world"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name)
      if [[ $# -gt 1 ]]; then
        name="$2"
        shift
      fi
      ;;
  esac
  shift
done

printf 'hello from bash, %s!\n' "$name"
