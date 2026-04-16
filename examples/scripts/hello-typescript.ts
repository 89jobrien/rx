#!/usr/bin/env bun

const args = process.argv.slice(2);
const flagIndex = args.indexOf("--name");
const name = flagIndex >= 0 && flagIndex + 1 < args.length ? args[flagIndex + 1] : "world";

console.log(`hello from typescript, ${name}!`);
