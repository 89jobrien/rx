# rx

`rx` installs `rust-script` scripts from:

- a local file
- a local directory
- a remote URL such as a GitHub raw file

Current MVP rules:

- only files with a `rust-script` shebang are accepted
- local directories are scanned recursively
- non-matching files are skipped when installing from a directory
- remote installs currently support single-file URLs

Example:

```bash
cargo run -- install scripts
cargo run -- install ./scripts/preflight.rs
cargo run -- install https://raw.githubusercontent.com/example/repo/main/script.rs
```
