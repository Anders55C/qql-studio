# QQL Studio

A macOS desktop app (Tauri 2 + Svelte 5) for filtering and rendering [QQL](https://qql.art) art seeds locally. You give it an ETH address, choose trait/characteristic filters, and it generates and renders matching seeds on your machine.

Built on top of [@wchargin](https://github.com/wchargin)'s `qqlrs` Rust renderer (vendored here as `qqlrs-main/`, with local modifications — see [Licensing](#licensing)).

## Repository layout

| Path | What it is |
|------|------------|
| `qqlrs-main/` | The QQL renderer (Rust lib + CLI). Third-party (@wchargin), MIT/Apache-2.0, **with local modifications** that surface extra layout/structure info for filtering. Don't break its golden tests. |
| `qql-studio/` | The Tauri desktop app. **This is the original code.** Rust backend in `src-tauri/`, Svelte 5 frontend in `src/`. |

`qql-studio` depends on `qqlrs-main` via a relative path (`qql = { path = "../../qqlrs-main" }`), so both folders must live together.

## Prerequisites

- **Rust** (stable, via [rustup](https://rustup.rs))
- **Node.js** 18+ and npm
- macOS (the only target wired up today). For Universal builds: `rustup target add x86_64-apple-darwin aarch64-apple-darwin`.

## Run (dev)

```sh
cd qql-studio
npm install
npm run tauri dev
```

## Build a shareable app

```sh
cd qql-studio
npm run tauri build -- --target universal-apple-darwin
```

Produces a `.app` and `.dmg` under `src-tauri/target/universal-apple-darwin/release/bundle/`. The build is **unsigned**, so first launch needs a right-click → Open (or `xattr -dr com.apple.quarantine "/Applications/QQL Studio.app"`).

## Tests

```sh
cd qqlrs-main && cargo test --release    # renderer (incl. 3 golden tests — must pass)
cd qql-studio/src-tauri && cargo test --lib
cd qql-studio && npm run check           # frontend type-check
```

## How it works (one paragraph)

QQL encodes 13 categorical traits in the last bytes of a 32-byte seed (ETH address + nonce + trait bits). `qqlrs-main` was extended to return rich `LayoutSummary` / `RenderData` (point counts, colors, radius buckets, quadrant distribution, and per-structure info for Formation / Orbital / Shadows pieces). The app's Rust backend filters candidate seeds in parallel against these characteristics and renders matches to PNG; the Svelte frontend drives the controls and a concurrency-limited render queue.

## Licensing

- `qqlrs-main/` retains its upstream dual license — see `qqlrs-main/LICENSE-MIT` and `qqlrs-main/LICENSE-APACHE`. Modifications are noted in code.
- `qql-studio/` — license TBD by the team.
