# Architecture Notes

## Port / Adapter Boundaries

`rx-core` defines the domain and owns the ports (traits). `rx-registry-json` provides
concrete adapters. CLI crates wire them together.

```
rx-core
  ├── RegistryStore (trait)        ← persistence port
  ├── RemoteScriptFetcher (trait)  ← fetch port
  └── ExecutionPlan (type)         ← output of planning

rx-registry-json
  ├── JsonRegistryStore            → implements RegistryStore
  ├── HttpScriptFetcher            → implements RemoteScriptFetcher
  └── xdg::default_paths()        → XDG path resolution

rx-install (thin CLI)
  └── wires JsonRegistryStore + HttpScriptFetcher → calls rx-core planning/install

rxx (thin CLI)
  └── wires HttpScriptFetcher → calls rx-core execution planning → executes plan
```

## Why This Split

- `rx-core` is testable without I/O: pass in-memory fakes for the two traits.
- `rx-registry-json` can be swapped for a different backend (SQLite, remote API) without
  touching domain logic.
- CLI crates stay thin and are not unit-tested in isolation — smoke/integration tests cover them.

## GitHub URL Normalisation

`rx-registry-json` rewrites blob URLs before fetch:

```
https://github.com/<user>/<repo>/blob/<ref>/<path>
→
https://raw.githubusercontent.com/<user>/<repo>/<ref>/<path>
```

This happens transparently; callers pass the original URL.

## Registry Format (v1)

```json
{
  "version": 1,
  "commands": [
    {
      "name": "preflight",
      "source": "./scripts/preflight.sh",
      "install_path": "/Users/alice/.config/rx/bin/preflight",
      "runtime": "sh",
      "description": null
    }
  ]
}
```

Reinstalling a script with the same derived name updates the existing entry in place.
