# rx-registry-json

JSON registry and HTTP fetcher adapters for the `rx` script installer.

## Purpose

This crate provides the two infrastructure adapters that `rx-install` and `rxx` plug in at
startup: a JSON-backed implementation of `RegistryStore` and a `reqwest`-based implementation
of `RemoteScriptFetcher`. It also resolves the XDG-style config paths used by those crates.

All behavior and domain logic lives in `rx-script-core`. This crate contains only I/O.

## Key Types

| Type / fn            | What it does                                                        |
| -------------------- | ------------------------------------------------------------------- |
| `RxPaths`            | Holds `root`, `bin_dir`, and `registry_path` as `PathBuf` fields   |
| `default_paths()`    | Resolves paths from `$XDG_CONFIG_HOME/rx` or `~/.config/rx`        |
| `JsonRegistryStore`  | Implements `RegistryStore` — reads and writes `registry.json`       |
| `ReqwestFetcher`     | Implements `RemoteScriptFetcher` — blocking HTTPS GET via `reqwest` |

The on-disk format is a versioned JSON object:

```json
{
  "version": 1,
  "commands": [
    {
      "name": "deploy",
      "source": "https://example.com/deploy.sh",
      "install_path": "/home/user/.config/rx/bin/deploy",
      "runtime": "bash",
      "description": null
    }
  ]
}
```

Commands are sorted by name on every write. Missing registry files are treated as empty
rather than an error.

## Usage

```rust
use rx_registry_json::{JsonRegistryStore, ReqwestFetcher, default_paths};
use rx_script_core::RegistryStore;

fn main() -> anyhow::Result<()> {
    let paths = default_paths()?;
    let store = JsonRegistryStore::new(paths.registry_path.clone());

    // List all installed commands
    for entry in store.list()? {
        println!("{}: {}", entry.name, entry.runtime);
    }

    Ok(())
}
```

## Workspace Role

```
rx-script-core          domain types, ports (RegistryStore, RemoteScriptFetcher)
rx-registry-json   <--  this crate: JSON + reqwest adapters, XDG path resolution
rx-install              rx CLI: install, list, run
rxx                     direct-run CLI
```

`rx-install` and `rxx` wire `JsonRegistryStore` and `ReqwestFetcher` together with the
planning functions from `rx-script-core`. Swap this crate to change persistence or fetch
strategy without touching domain logic.
