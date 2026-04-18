# Supported Runtimes

Current shebang families accepted by `rx`, and their registry `runtime` tag:

| Shebang prefix | `runtime` tag | Interpreter binary |
|----------------|---------------|--------------------|
| `#!/usr/bin/env rust-script` | `rs` | `rust-script` |
| `#!/usr/bin/rust-script` | `rs` | `rust-script` |
| `#!/usr/bin/env python3` | `py` | `python3` |
| `#!/usr/bin/env python` | `py` | `python` |
| `#!/usr/bin/python3` | `py` | `python3` |
| `#!/usr/bin/python` | `py` | `python` |
| `#!/usr/bin/env node` | `js` | `node` |
| `#!/usr/bin/env bun` (`.js`) | `js` | `bun` |
| `#!/usr/bin/env bun` (`.ts`) | `ts` | `bun` |
| `#!/usr/bin/env bash` | `sh` | `bash` |
| `#!/usr/bin/env zsh` | `zsh` | `zsh` |
| `#!/usr/bin/env fish` | `fish` | `fish` |
| `#!/usr/bin/env nu` | `nu` | `nu` |
| `#!/usr/bin/env ruby` | `rb` | `ruby` |

When adding a new runtime, add a row here and update `README.md` in the same commit.
