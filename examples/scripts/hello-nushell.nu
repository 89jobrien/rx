#!/usr/bin/env nu

def main [
  --name: string = "world"
] {
  print $"hello from nushell, ($name)!"
}
