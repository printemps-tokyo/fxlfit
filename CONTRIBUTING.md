# Contributing

Thanks for your interest in contributing to fxlfit.

## Development

1. Fork / clone the repository
2. Install a stable Rust toolchain (https://rustup.rs)
3. Create a branch: `git switch -c feat/your-change`
4. Make your change and verify locally:
   ```bash
   cargo fmt --all -- --check
   cargo clippy --all-targets -- -D warnings
   cargo test --all
   cargo build --release
   ```
5. Commit and open a pull request

## Ground rules

- A new check needs a primary source. Add it to `src/checks/catalog.rs` with a
  link to the spec section or W3C note it comes from and the date you read it
  -- not a blog post, not a model's memory. A rule nobody can look up is a rule
  nobody can argue with.
- Check ids are a public interface. `FXL003` means one thing forever: `--only`
  and `--skip` selectors, CI configurations and JSON consumers are pinned to
  them. Retire an id rather than repurposing it.
- Severity follows the consequence, not the annoyance. `error` is for a book
  that is broken or will be rejected; `warn` is for something a reader or a
  store will notice; `info` is for something worth knowing that never blocks a
  release. If you are unsure, it is a warning.
- Say what was found, not what the author meant. Every finding quotes its
  evidence -- the string, the size, the page. Heuristics (alt text shape, the
  fixed-layout inference) are warnings that name the pattern they matched.
- No store limits in the code. File-size ceilings and required levels change
  per retailer and per year; they stay flags with documented defaults, so the
  tool never claims something on a store's behalf that it cannot source.
- Nothing leaves the machine and nothing is decoded. No network calls, and
  image headers only -- a preflight tool is pointed at files that arrived from
  somewhere else.

## Tests

The suite is fixture-free: every EPUB is built in code as a real ZIP container
in `tests/cli.rs`, and each test breaks exactly one thing about an otherwise
clean book. Follow that pattern; a checked-in binary fixture needs a very good
reason. Parsers (viewport, conformance string, image headers, path resolution)
are unit-tested next to the code they belong to.
