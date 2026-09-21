# rx-runner

`rx-runner` is a small, runtime-neutral library for driving child processes from polling event
loops. It captures stdout and stderr on background reader threads, turns them into tagged lines,
bounds unread output, exposes non-blocking status polling, and supports cancellation.

The implementation uses only the Rust standard library. It does not require Tokio or another
async runtime.

## Current Workspace Role

This crate is an early standalone workspace member. No other `rx` workspace crate currently
depends on it: the `rx` and `rxx` binaries still spawn processes through their own code paths.
Treat the API documented here as the current library surface, not as a claim that `rx` CLI command
execution already uses it.

## Usage

```rust
use rx_runner::{CommandSpec, OutputStream};
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let mut process = CommandSpec::new("cargo")
        .args(["check", "--workspace"])
        .current_dir("/path/to/project")
        .output_capacity(2_000)
        .spawn()?;

    loop {
        let update = process.poll()?;
        if update.dropped_lines > 0 {
            eprintln!("dropped {} output lines", update.dropped_lines);
        }
        for line in update.lines {
            match line.stream {
                OutputStream::Stdout => println!("{}", line.text),
                OutputStream::Stderr => eprintln!("{}", line.text),
            }
        }
        if let Some(exit) = update.exit {
            println!("exit {:?}, success={}, elapsed={:?}", exit.code, exit.success, exit.elapsed);
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    Ok(())
}
```

`CommandSpec` keeps the executable and each argument separate; it does not parse a shell command
string. `spawn` pipes stdout and stderr while leaving the child's other process settings at
`std::process::Command` defaults.

## Public API

### `CommandSpec`

`CommandSpec::new(program)` creates a command with no arguments, no explicit working directory,
and `DEFAULT_OUTPUT_CAPACITY` (1,024 lines). Builder methods provide:

- `arg` and `args` for separate process arguments
- `current_dir` for the child working directory
- `output_capacity` for the combined unread stdout/stderr line buffer
- `spawn` to create a `RunningProcess`

The `program`, `arguments`, and `working_directory` accessors expose the configured values.

### Output Types

| Type            | Purpose                                                                     |
| --------------- | --------------------------------------------------------------------------- |
| `OutputStream`  | Distinguishes `Stdout` from `Stderr`                                        |
| `OutputLine`    | Contains a stream tag and decoded line text                                 |
| `ProcessUpdate` | Returns drained lines, a dropped-line count, and optional terminal status   |
| `ProcessExit`   | Contains optional platform code, success state, and wall-clock elapsed time |

Reader threads decode with `String::from_utf8_lossy` and remove trailing CR/LF bytes. Ordering
between stdout and stderr reflects reader-thread scheduling and is not a strict merge of the
child's write order.

The output capacity applies to lines not yet drained by `poll`. When full, the oldest retained line
is discarded and counted in the next update's `dropped_lines`. Capacity zero discards every line
while preserving the count.

### `RunningProcess`

- `id()` returns the platform process identifier.
- `poll()` drains current output and checks child status without waiting for the child to exit.
- `cancel()` kills a still-running direct child, waits for it, and returns its `ProcessExit`.

Terminal status from `poll` is emitted once. `cancel` returns terminal status directly and marks it
reported, so later polls do not repeat it. Dropping an unfinished `RunningProcess` attempts to kill
and reap the direct child. The crate does not promise process-group or descendant-tree cleanup.

## Features and Dependencies

The crate has no feature flags and no third-party dependencies. Its public errors are
`std::io::Error` values.

## Development and Testing

From the workspace root:

```bash
cargo check -p rx-runner
cargo fmt --all -- --check
cargo clippy -p rx-runner --all-targets -- -D warnings
cargo test -p rx-runner
cargo test --doc -p rx-runner
```

Unit tests exercise argument preservation, working directories, stdout/stderr tagging, bounded
output and dropped-line counts, cancellation, one-time exit reporting, and the case where a
descendant keeps an inherited output pipe open. Cancellation coverage is Unix-specific where the
fixture relies on `sleep` and shell `exec` behavior.
