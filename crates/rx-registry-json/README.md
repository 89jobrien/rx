# rx-registry-json

`rx-registry-json` provides the concrete storage, HTTP, and filesystem adapters used by the `rx`
workspace. Domain rules and adapter traits live in `rx-core`; this crate implements those traits
with JSON, blocking `reqwest`, `std::fs`, and `walkdir`.

## Workspace Role

```text
rx-core              domain types, ports, and execution planning
rx-registry-json     JSON, HTTP, and filesystem adapters (this crate)
rx-install           wires the adapters into the rx CLI
rx-rxx               uses FsScriptReader for the rxx CLI
```

`rx-install` uses every adapter in this crate. `rx-rxx` only uses `FsScriptReader`; direct runs do
not read or write the registry.

## Public API

| API                 | Implemented port or behavior                                   |
| ------------------- | -------------------------------------------------------------- |
| `RxPaths`           | Holds the rx config root, install directory, and registry path |
| `default_paths()`   | Resolves the default XDG-style paths                           |
| `JsonRegistryStore` | Implements `rx_core::RegistryStore`                            |
| `ReqwestFetcher`    | Implements `rx_core::RemoteScriptFetcher`                      |
| `FsScriptWriter`    | Implements `rx_core::ScriptWriter`                             |
| `WalkdirScanner`    | Implements `rx_core::DirectoryScanner` recursively             |
| `FsScriptReader`    | Implements `rx_core::ScriptReader`                             |

`ReqwestFetcher` performs a blocking HTTP GET, rejects non-success status codes, and decodes the
response body as text. TLS uses `rustls`; `reqwest` default features are disabled.

`FsScriptWriter` creates the install directory, writes `install_dir/<name>`, and sets mode `0755`
on Unix. `WalkdirScanner` returns every file recursively. It does not filter script types; runtime
validation remains in `rx-core`.

## Default Paths

`default_paths()` first checks `XDG_CONFIG_HOME`, then falls back to `HOME`:

| Field           | With `XDG_CONFIG_HOME=/config` | With `HOME=/home/alice`                |
| --------------- | ------------------------------ | -------------------------------------- |
| `root`          | `/config/rx`                   | `/home/alice/.config/rx`               |
| `bin_dir`       | `/config/rx/bin`               | `/home/alice/.config/rx/bin`           |
| `registry_path` | `/config/rx/registry.json`     | `/home/alice/.config/rx/registry.json` |

Resolution fails if neither environment variable is available.

## Registry Behavior

`JsonRegistryStore::new` accepts any registry path. Listing a missing file returns an empty list.
An upsert creates the parent directory, replaces entries with the same command name, preserves
other entries, sorts commands by name, and writes pretty-printed JSON.

The format is versioned and currently uses `version: 1`:

```json
{
  "version": 1,
  "commands": [
    {
      "name": "deploy",
      "source": "https://example.com/deploy.sh",
      "install_path": "/home/alice/.config/rx/bin/deploy",
      "runtime": "sh",
      "description": null
    }
  ]
}
```

Runtime values are the serialized `rx_core::Runtime` codes: `rs`, `py`, `js`, `ts`, `sh`, `zsh`,
`fish`, `nu`, and `rb`. New upserts currently set `description` to `null`.

## Usage

The trait must be in scope to call `list` or `upsert`:

```rust
use rx_core::RegistryStore;
use rx_registry_json::{JsonRegistryStore, default_paths};

fn main() -> anyhow::Result<()> {
    let paths = default_paths()?;
    let store = JsonRegistryStore::new(paths.registry_path);

    for entry in store.list()? {
        println!("{} -> {}", entry.name, entry.install_path.display());
    }
    Ok(())
}
```

There are no crate feature flags. HTTP support and all filesystem adapters are included in the
default build.

## Development

From the workspace root:

```bash
cargo check -p rx-registry-json
cargo fmt --all -- --check
cargo clippy -p rx-registry-json --all-targets -- -D warnings
cargo test -p rx-registry-json
```

The integration tests enable `rx-core/test-support` and run its conformance suites against the JSON
store, filesystem reader and writer, and recursive scanner. `ReqwestFetcher` is not exercised by
that integration test because it requires a live HTTP endpoint.
