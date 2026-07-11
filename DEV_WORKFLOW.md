# Dev workflow: build on monolith, run on the Pi

monzoboiii runs in anger on an always-on Raspberry Pi, but the Pi is painfully slow to compile on (axum + tokio + reqwest from scratch takes a long time on Pi-class hardware). This guide sets up a workflow where all building happens on your dev machine ("monolith") and the Pi only ever receives finished binaries.

**The Pi no longer holds a source checkout, `Cargo.toml`, or a Rust toolchain.** It has exactly one directory, `/home/ben/monzoboiii-run/`, containing:

```
monzoboiii-run/
├── bin/
│   ├── monzoboiii      # the server binary
│   └── monzoctl        # the diagnostics/CLI binary
└── tokens.toml          # live OAuth tokens, read/written by the server
```

Everything else — source, `Cargo.lock`, `justfile`, git history — lives only on monolith. `config.toml` stays where it always was, at `/home/ben/.config/monzoboiii/config.toml` on the Pi (untouched by any of this).

---

## One-time setup (already done on this machine, kept here for reference)

1. **SSH access.** `ssh raspberrypi` must work with key auth, and `sudo` on the Pi must be passwordless (`ben ALL=(ALL) NOPASSWD: ALL` in sudoers). Both were already true here — nothing to configure.

2. **Install `cross`** — the cargo subcommand that cross-compiles inside a container so you don't need a Pi-targeting linker/glibc installed locally:

   ```bash
   cargo install cross --locked
   ```

3. **Container engine.** `cross` defaults to Docker, but this user account isn't in the `docker` group (and adding it would need a relogin). We use **podman** instead, which runs rootless with no extra permissions needed. This is set via `CROSS_CONTAINER_ENGINE=podman` in every `justfile` recipe that calls `cross` — you don't need to export it yourself.

4. **The `aarch64-unknown-linux-gnu` rust target** was added locally with `rustup target add aarch64-unknown-linux-gnu`. Not strictly required (the target lives inside `cross`'s container), but keeps rust-analyzer aware of the target for editing.

> **Why podman over Docker, and why `cross` over a plain gcc cross-linker?**
> `reqwest` already uses `rustls-tls` (no OpenSSL), so a plain `aarch64-linux-gnu-gcc` toolchain would work too — but Fedora's glibc is newer than Debian bookworm's (the Pi's OS), so a natively-linked binary risks `GLIBC_2.XX not found` at runtime on the Pi. `cross` builds inside a container pinned to an older glibc baseline, avoiding that mismatch entirely.

---

## Everyday deploy: `just reload`

```bash
just reload
```

This cross-compiles release binaries for `aarch64-unknown-linux-gnu`, rsyncs `monzoboiii` and `monzoctl` to `/home/ben/monzoboiii-run/bin/` on the Pi, and restarts the systemd service. Typical time: well under a minute once the container image is cached (the first-ever run pulls a ~1-2GB image, so expect a few minutes then).

Other recipes:

| Command | What it does |
|---|---|
| `just build` | Native x86_64 build, for local testing/`cargo run` on monolith |
| `just build-pi` | Cross-compile only, no deploy |
| `just push-pi` | Cross-compile + rsync binaries, no restart |
| `just provision-pi` | (Re-)install the systemd unit from `deploy/monzoboiii.service` — only needed if that file changes |
| `just diagnose-pi` | Runs `monzoctl diagnose` on the Pi over SSH |
| `just logs-pi` | Tails `journalctl -u monzoboiii -f` on the Pi |

The systemd unit itself is checked into the repo at `deploy/monzoboiii.service` — edit it there and run `just provision-pi` to push changes, rather than hand-editing it on the Pi.

---

## Testing real webhooks against your local dev build

The Pi's production instance is what Monzo's webhooks actually hit (via whatever public tunnel is set up per `GET_CONNECTED.md` Step 8 — the tunnel always points at `localhost:<port>` on the Pi). To debug with real transaction events without touching the public URL or re-registering the webhook:

```bash
just webhook-tunnel
```

This:
1. Stops the Pi's production `monzoboiii` service (freeing its port).
2. Opens an SSH remote port-forward (`-R <port>:localhost:<port>`), so anything hitting that port **on the Pi** — which is exactly what the Pi's local tunnel client (cloudflared/ngrok) connects to — gets relayed to the same port **on monolith**.
3. Restores the production service automatically when you `Ctrl-C` (via a shell trap), even if you forget.

While it's running, start your dev build in another terminal:

```bash
cargo run
```

Make sure your local `config.toml`/`tokens.toml` match what the Pi uses (copy them down, or point `--config` at a personal dev config) — real transactions will hit your local build's webhook handler with the same secret path Monzo already has registered.

The port defaults to `3687` (the Pi's current configured port — check `[app] port` in `/home/ben/.config/monzoboiii/config.toml` if this ever changes). Override with `just webhook-tunnel port=XXXX`.

> **Why not just run a second tunnel (ngrok/cloudflared) pointed at monolith?** That would mean registering a second webhook and juggling two URLs. Relaying through the Pi's existing tunnel means the exact same public URL and secret keep working — only the backend flips, and flipping back is automatic.

---

## Non-reproducible Pi state that was preserved during setup

When the Pi's project directory was cleaned up, two things were **not** deleted — they were copied to monolith at `pi-payloads-backup/` (gitignored) before the Pi's copies were removed:

- `payloads/` — 43 captured real Monzo webhook payloads, used for the category/pattern analysis in `next_steps.txt`
- `.env` — a live Monzo OAuth `CLIENT_ID`/`CLIENT_SECRET` pair

`tokens.toml` (the live token pair) was moved, not copied, straight into `/home/ben/monzoboiii-run/` — it was never duplicated or at risk.
