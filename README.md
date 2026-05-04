![Alt text](stuff/hero.png "Patch-Me-Devs")
<h1 align="center">Patch-Me-Devs</h1>

<p align="center">An open source PixelWorlds bot — released so the devs finally have a reason to patch it.</p>

## Why I made this public

The developers of this game never gave a damn whether someone could build a fully working bot or not. They had every opportunity to patch these vulnerabilities and they chose to ignore it. So here it is — a complete, working source. It's out in the open now. Patch it. It's not even that hard.

This is not about cheating. This is a challenge to the devs: if a working bot can be built and shared publicly, you have no excuse not to fix it.

## Overview

A Rust-powered backend with a web dashboard frontend for managing multiple PixelWorlds sessions from one place. Includes live logs, session orchestration, world controls, inventory actions, minimap rendering, and gameplay automation.

## Run

```bash
cd web
bun install
bun run build
cd ..
cargo run --bin Moonlight
```

Then open `http://127.0.0.1:3000`.

## Note

This is for educational and patch-reference purposes only. No AC bypass is included. Use it to understand the vulnerabilities — and if you're a dev, use it to fix them.

Contributions are welcome. If you find something else worth patching, open a PR.
