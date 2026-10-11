# Contributing

Fetch the pinned schemas, run `cargo fmt`, `cargo test --locked` and
`cargo clippy --all-targets --locked -- -D warnings` before opening a pull request.
Add regression coverage for protocol changes, permissions and monetary state changes.
Check responses against both API layers. Do not introduce success stubs for unsupported RPCs.
Never commit data directories, auth keys, sessions, login codes or private RSA keys.
By contributing original code you agree to license that contribution under MIT.
