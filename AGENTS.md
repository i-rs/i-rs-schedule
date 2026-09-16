# AGENTS.md

## Stack
- Rust, edition 2024, toolchain stable
- Workspace: crates/schedule (server) + crates/cli
- Frontend: React + Vite + Tailwind v4 (frontend/)

## Commands
- `cargo build` / `cargo run -p i-rs-schedule` / `cargo test`
- `cargo clippy` for linting (zero warnings is the bar)
- `cargo fmt` for formatting
- Frontend: `cd frontend && pnpm build / pnpm lint / pnpm dev`
- CI: GitHub Actions runs fmt/clippy/test/build + frontend lint/build on push/PR

## Notes
- Tests live as `#[cfg(test)]` modules next to the code; run with `cargo test`
- Config: env vars (SCHEDULE_DB/PORT/TOKEN, RETENTION_DAYS, RUST_LOG) override optional `config.toml`
