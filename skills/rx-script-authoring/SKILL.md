---
name: rx-script-authoring
description: >
  Provides templates and rules for writing rx-compatible scripts. Covers supported shebangs,
  command naming, and common mistakes. Triggers on "write an rx-compatible script",
  "make a script for rx", "what shebang does rx accept", "write a rust-script for rx",
  "write a nu script for rx", "create a script I can install with rx".
version: 0.1.0
---

# Writing rx-Compatible Scripts

`rx` accepts scripts based on their shebang line. If the first line matches a supported
shebang family, the script is compatible. No extension detection — shebang only.

## Supported Shebangs

| Runtime | Shebang | Registry tag |
|---------|---------|--------------|
| Rust (rust-script) | `#!/usr/bin/env rust-script` | `rs` |
| Python 3 | `#!/usr/bin/env python3` | `py` |
| Node.js | `#!/usr/bin/env node` | `js` |
| Bun (JS) | `#!/usr/bin/env bun` | `js` |
| Bun (TS) | `#!/usr/bin/env bun` | `ts` |
| Bash | `#!/usr/bin/env bash` | `sh` |
| Zsh | `#!/usr/bin/env zsh` | `zsh` |
| Fish | `#!/usr/bin/env fish` | `fish` |
| Nushell | `#!/usr/bin/env nu` | `nu` |
| Ruby | `#!/usr/bin/env ruby` | `rb` |

## Script Templates

### Rust (rust-script)

```rust
#!/usr/bin/env rust-script
//! ```cargo
//! [dependencies]
//! # add crates here
//! ```

fn main() {
    println!("Hello from rust-script");
}
```

### Nushell

```nu
#!/usr/bin/env nu

def main [] {
    echo "Hello from nu"
}
```

### Bash

```bash
#!/usr/bin/env bash
set -euo pipefail

echo "Hello from bash"
```

### Python

```python
#!/usr/bin/env python3

def main():
    print("Hello from python3")

if __name__ == "__main__":
    main()
```

### Node.js

```js
#!/usr/bin/env node

console.log("Hello from node");
```

## Command Naming

The installed command name comes from the **filename stem**. `preflight.sh` installs as
`preflight`. `check.rs` installs as `check`.

Collision rule: `foo.sh` and `foo.rs` share the name `foo` — reinstalling one overwrites
the other's registry entry. Choose distinct stem names when installing from a directory.

## Making a Script Executable

On Unix, mark the script executable before installing from a local path:

```bash
chmod +x my-script.sh
```

`rx install` does this automatically on installation, but having it pre-set is good practice.

## Testing a Script Before Installing

To test a script without adding it to the registry, run it directly with `rxx`:

```bash
rxx ./my-script.sh
rxx ./my-script.sh -- --help
```

`rxx` applies the same shebang validation as `rx install` and fails immediately on an
unsupported shebang. Note: `rxx` **executes** the script — use it only when the script is
safe to run.

## Common Mistakes

- **Missing shebang**: A file with no first-line shebang is rejected.
- **Wrong shebang path**: `#!/usr/bin/python3` is accepted; `#!/usr/local/bin/python3` is not
  (only the `env`-form and `/usr/bin` forms are on the allowlist).
- **Stem collision**: Two scripts with the same stem in a directory install — the second
  overwrites the first in the registry.
