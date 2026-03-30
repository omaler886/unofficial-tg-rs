# Research Notes

## Official sources

- Telegram Desktop: <https://github.com/telegramdesktop/tdesktop>
- Telegram Android: <https://github.com/DrKLO/Telegram>
- Telegram iOS: <https://github.com/TelegramMessenger/Telegram-iOS>
- TDLib: <https://github.com/tdlib/td>
- Telegram file API docs: <https://core.telegram.org/api/files>

## GitHub implementations reviewed

### `grammers`

Repo: <https://github.com/Lonami/grammers>

Useful ideas:

- concurrent download workers for large files
- `upload.getFile` chunk scheduling with shared part cursors
- multi-worker `saveBigFilePart` uploads for files larger than 10 MB
- a simple Rust-native pattern for splitting large transfers into fixed parts

Selected takeaways for this rewrite:

- keep upload/download acceleration in a dedicated module
- use bounded worker counts instead of unbounded task fan-out
- parallelize big-file uploads only
- pre-size download sinks and write by offset

### `gotd/td`

Repo: <https://github.com/gotd/td>

Useful ideas:

- configurable thread counts for both uploader and downloader
- explicit validation of Telegram part size rules
- separation of planner, reader, verifier, and write loops
- CDN-aware download handling and retry/reporting hooks

Selected takeaways for this rewrite:

- expose acceleration as policy plus hints, not as hard-coded magic numbers
- keep verification and retry paths separate from the data plane
- model download sinks as writer-at targets rather than append-only streams

### `tg-upload`

Repo: <https://github.com/TheCaduceus/tg-upload>

Useful ideas:

- workflow affordances around upload/download tooling
- file splitting/combining UX and progress expectations

Important limitation stated by that project:

- it explicitly notes that transfer speed depends on Telegram server behavior, premium limits, and the user's connection

That means it is a product reference, not the main acceleration algorithm source for this repo.

## Architecture decision

This repo treats accelerated upload/download as an add-on subsystem for a Rust rewrite, not as the application's core identity.

The implementation choices here are:

1. Keep Telegram protocol rules in `tg-core`.
2. Keep acceleration heuristics and the concurrent runtime in `tg-transfer`.
3. Keep TDLib bootstrapping separate in `tg-tdlib`.
4. Expose a single planning surface from `tg-app`.
5. Ship a CLI first so the transfer layer can be verified before a desktop/mobile shell is attached.

## Deliberate non-goals for this commit

- fully reimplementing MTProto from scratch
- copying official client code into the repo
- claiming impossible speed gains that ignore Telegram-side limits

The goal of this first repo state is a clean Rust rewrite foundation with a transfer acceleration feature that is grounded in official rules and proven open-source patterns.
