# Unofficial TG RS

`Unofficial TG RS` is a brand-new Rust rewrite workspace for a cross-platform Telegram client with transfer acceleration as an add-on feature, not the protocol core.

This repository does **not** patch official clients in place. Instead it starts from a clean Rust architecture and keeps upload/download acceleration isolated inside a dedicated transfer engine so it can be wired into desktop, mobile, or CLI surfaces later.

## What is in this repo

- `crates/tg-core`: shared Telegram transfer rules, limits, job models, and plan structs
- `crates/tg-transfer`: concurrent upload/download planner and runtime inspired by existing GitHub implementations
- `crates/tg-tdlib`: TDLib bootstrap and discovery config for the future app shell
- `crates/tg-app`: rewrite manifest and service layer that ties sources, plans, and TDLib together
- `apps/tg-cli`: a runnable CLI for generating plans and simulating accelerated transfers

## Why the acceleration feature is separate

The official Telegram API and open-source clients already define the rules for:

- file chunk sizing
- big-file upload thresholds
- precise range download windows
- queue and connection-based parallelism
- premium-triggered transfer limits

The acceleration work here is therefore implemented as a reusable Rust transfer engine that can sit on top of a clean-room rewrite instead of being treated as the application's core identity.

## Research baseline

The current design is based on:

- official Telegram source repositories and file API docs
- Rust `grammers` concurrent upload/download code
- Go `gotd/td` uploader/downloader architecture
- GitHub-side product tools such as `tg-upload`, mainly as workflow reference

See [docs/research.md](D:\New project\unofficial-tg-rs\docs\research.md) for the concrete links and the design choices extracted from them.

## Quickstart

```powershell
cargo run -p tg-cli -- manifest
cargo run -p tg-cli -- plan --direction upload --size 1610612736 --policy aggressive --premium
cargo run -p tg-cli -- simulate --direction download --size 8388608 --policy balanced
```

## Current status

This repo already includes:

- a full Rust workspace from scratch
- transfer planning rules for upload/download acceleration
- a concurrent transfer runtime with mock backends
- GitHub Actions for lint, test, and release builds

This repo does **not** yet include:

- a production Telegram auth/session implementation
- a finished desktop/mobile UI shell
- an actual MTProto or TDLib-backed accelerated transfer adapter

Those pieces are intentionally isolated so they can be added without rewriting the transfer layer again.

## Naming note

Telegram's branding rules require unofficial apps to make their status obvious. This repository therefore uses the `Unofficial` prefix and does not claim to be an official Telegram client.
