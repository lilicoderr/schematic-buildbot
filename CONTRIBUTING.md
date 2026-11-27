# Contributing

Contributions are welcome, including small ones. 한국어 PR/이슈도 환영합니다.

## Development

```bash
cargo test --workspace                                 # unit tests, no server needed
cargo clippy --workspace --all-targets -- -D warnings  # CI runs this too
cargo run -q --bin build-bot -- x x blueprints/stair-tower.txt --dry-run
python tools/preview.py blueprints/cottage.txt -o /tmp/c.png
```

For live checks run a local vanilla 1.21.1 server with `online-mode=false` and `gamemode=creative`.

## Rules

- Never commit credentials, tokens or server addresses.
- Protocol changes: cite the packet-id source (e.g. minecraft-data) in the PR.
- New features need unit tests that run without a server. User-facing text goes through `tr!("English", "한국어")`.
- Say honestly what you verified on a live server and what you only unit-tested.
- Only use the bot on servers whose operator allows it.

## Ideas

- More block-state families: trapdoors, buttons/levers, 6-way blocks (dispensers, pistons), hinges.
- Another Minecraft version: follow "Adding a release" in [docs/RE/versions.md](docs/RE/versions.md).
- Survival mode with materials from chests.
