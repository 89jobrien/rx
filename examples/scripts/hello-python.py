#!/usr/bin/env python3

import sys


def resolve_name(argv: list[str]) -> str:
    for index, arg in enumerate(argv):
        if arg == "--name" and index + 1 < len(argv):
            return argv[index + 1]
    return "world"


print(f"hello from python, {resolve_name(sys.argv[1:])}!")
