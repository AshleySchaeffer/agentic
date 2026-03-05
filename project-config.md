# Project: agentic

<project-config>
<languages>
Rust (edition 2024)
</languages>

<build>
cargo build --release
</build>

<test>
cargo test
</test>

<verification>
cargo clippy -- -D warnings
cargo fmt --check
cargo test
</verification>

<key-paths>
src/
Cargo.toml
Cargo.lock
</key-paths>
</project-config>
