# Getting connected to Monzo

This guide takes you from a fresh clone to a working setup where monzoboiii automatically moves money from your spending pots every time a transaction fires.

## How it works (the big picture)

monzoboiii runs as a small HTTP server. Monzo sends a webhook POST to it every time a transaction hits your account. The server looks at the transaction's spending category, finds the pot with the matching name, and moves the exact transaction amount from that pot back to your main account — so your balance stays topped up from the right pot automatically.

Three things must be in place before any of this works:

- **Auth tokens** — so monzoboiii can call the Monzo API on your behalf
- **Pots with category names** — so there is something to withdraw from
- **A registered webhook** — so Monzo knows where to POST transaction events

---

## Step 1 — Build the binaries

```bash
cargo build --release
```

This produces two binaries:

- `target/release/monzoboiii` — the server
- `target/release/monzoctl` — a CLI with `diagnose` and `webhooks` commands for checking and managing the setup

---

## Step 2 — Create a Monzo developer client

Monzo requires an OAuth 2.0 client to allow any third-party app (including your own) to access your account. You create this once in the developer portal.

1. Go to **https://developers.monzo.com** and log in with your Monzo email address.
2. Click **Clients → New client**.
3. Fill in the form:
   - **Name** — anything, e.g. `monzoboiii`
   - **Redirect URL** — `http://YOUR_LAN_IP:3000/auth/callback`
     Use the local IP of this machine (e.g. `http://192.168.1.10:3000/auth/callback`).
     If you will be opening the browser on this machine itself, `http://localhost:3000/auth/callback` also works.
   - **Confidentiality** — **Confidential**
4. Save. Note the **Client ID** and **Client Secret** shown on the next screen — you will need them in the next step.

> **Why "Confidential"?** A confidential client has a client secret that only your server sees. This is the correct type for a server-side app that stores credentials securely. A "public" client (for mobile or browser apps) cannot safely hold a secret.

> **Why must the redirect URL match exactly?** During OAuth, after you approve the app in Monzo, Monzo redirects your browser back to this URL. If the URL doesn't match what was registered, Monzo rejects the redirect entirely — it is a security measure to prevent code-theft attacks.

---

## Step 3 — Create the config file

```bash
mkdir -p /home/ben/.config/monzoboiii
cp config.toml.example /home/ben/.config/monzoboiii/config.toml
nano /home/ben/.config/monzoboiii/config.toml
```

Fill it in:

```toml
[app]
secret = "a-long-random-string-you-invent"
port = 3000
redirect_uri = "http://YOUR_LAN_IP:3000/auth/callback"

[monzo]
client_id = "..."        # from the developer portal
client_secret = "..."    # from the developer portal
account_id = "acc_..."   # your Monzo account ID — Step 6 explains how to find this
```

**`secret`** — This string is embedded in every webhook URL that Monzo calls (`/webhook/monzo/YOUR_SECRET`). Your server checks it before processing any request, so someone who discovers your public hostname cannot trigger pot withdrawals without also knowing the secret. Treat it like a password and make it long and random.

**`redirect_uri`** — Must match exactly what you entered in the developer portal in Step 2.

**`account_id`** — Every Monzo account has an opaque ID starting with `acc_`. Leave a placeholder for now; Step 6 shows you how to find the real value.

---

## Step 4 — Start the server

Run from the project directory (this is important — `tokens.toml` is written relative to where you run the binary):

```bash
cd /home/ben/monzoboiii
./target/release/monzoboiii
```

You should see `Listening on :3000`. The line about the pot-map refresh failing is expected — there are no auth tokens yet, so the API call cannot succeed. That warning will go away after Step 5.

---

## Step 5 — Authenticate with Monzo

Open a browser that can reach this machine and navigate to:

```
http://YOUR_LAN_IP:3000/auth/reauth
```

**What happens:**
1. The server redirects your browser to `https://auth.monzo.com/` with your `client_id` embedded in the URL.
2. Monzo sends a "magic link" email to the address on your Monzo account.
3. You click the link in the email, which opens the Monzo app (or web) and shows an authorisation screen.
4. You tap **Approve** in the Monzo app.
5. Monzo redirects your browser back to the `redirect_uri` — hitting your server's `/auth/callback` endpoint.
6. The server exchanges the one-time code for access and refresh tokens, and saves them to `tokens.toml` in the project directory.
7. Your browser shows "Authenticated — you can close this tab."

> **Why a magic link?** Monzo's OAuth always requires an email confirmation step. This proves to Monzo that the account owner (not just someone who knows the password) is authorising access. The magic link is not something monzoboiii controls — it is Monzo's own security policy.

> **Where are the tokens stored?** In `tokens.toml` in the directory you ran the binary from. This file is in `.gitignore`. The access token expires periodically; monzoboiii automatically refreshes it using the refresh token, and overwrites `tokens.toml` with the new pair.

---

## Step 6 — Find and confirm your account ID

Run the diagnostics tool:

```bash
./target/release/monzoctl diagnose
```

If the `account_id` placeholder in your config doesn't match your real account, you will see something like:

```
[FAIL] Configured account_id (acc_placeholder) not found in your accounts
         → Your account IDs: acc_0000AbCdEfGhIjKlMn01
         → Update account_id in /home/ben/.config/monzoboiii/config.toml and restart the server
```

Update the config with the correct ID and restart the server.

> **Why does the app need to know the account ID?** Two reasons: (1) the Monzo API scopes pot and webhook queries to a specific account, and (2) the webhook handler verifies that every incoming transaction event belongs to this account before acting on it — so a webhook for a joint account or a different Monzo account is silently ignored.

---

## Step 7 — Create spending pots in the Monzo app

In the Monzo app, create one or more pots whose names **exactly match** a Monzo spending category. monzoboiii only recognises pots with these exact names (lowercase):

```
general        eating_out     expenses       transport
cash           bills          entertainment  shopping
holidays       groceries      family         charity
personal_care  savings
```

For example, create a pot called `groceries`. When a grocery transaction fires, monzoboiii will move the exact transaction amount from that pot back to your main account.

> **Why exact match on category name?** Monzo tags every transaction with one of these category strings. monzoboiii refreshes a map of `category → pot_id` from the API every 3 hours, built solely from pot names. If no pot has the matching name the webhook is acknowledged with HTTP 200 and nothing happens — so you opt in only to the categories you care about.

> **Why does the money move back to the main account?** Monzo pots are separate from your main balance. When you spend on your card, the transaction always hits the main account balance. The pot withdrawal then "reimburses" the main account for that spend, giving the illusion that the money came from the pot.

---

## Step 8 — Expose the server to the internet

Monzo's servers need to reach your webhook endpoint over HTTPS. A few options:

### ngrok (quick, URL changes on free-tier restart)

```bash
ngrok http 3000
```

Note the `https://xxxx.ngrok-free.app` URL.

### cloudflared Quick Tunnel (free, semi-stable URL)

```bash
cloudflared tunnel --url http://localhost:3000
```

Note the `https://xxxx.trycloudflare.com` URL.

### Cloudflare Tunnel with a custom domain (permanent — recommended for ongoing use)

If you own a domain on Cloudflare, a named tunnel gives you a stable `https://monzoboiii.yourdomain.com`. See the [Cloudflare Tunnel docs](https://developers.cloudflare.com/cloudflare-one/connections/connect-apps/) for setup.

> **Why HTTPS?** Monzo only delivers webhooks to HTTPS endpoints. Both ngrok and cloudflared provide HTTPS termination automatically, so your server still listens on plain HTTP locally.

> **Why not just open a port on the router?** You can, but you would need a TLS certificate (e.g. via Let's Encrypt with a domain) and you expose your home IP. A tunnel avoids both problems.

---

## Step 9 — Register the webhook with Monzo

Monzo must be told where to send transaction events. Run the diagnostics tool again and it will print the exact `curl` command with your token and account ID filled in:

```bash
./target/release/monzoctl diagnose
```

You will see something like:

```
[WARN] No webhook registered matching your secret
         → Register one with this curl command (replace YOUR_PUBLIC_HOST):

  curl -X POST https://api.monzo.com/webhooks \
    -H 'Authorization: Bearer eyJhbGci...' \
    -d 'account_id=acc_0000AbCd...' \
    -d 'url=https://YOUR_PUBLIC_HOST/webhook/monzo/your-secret'
```

Replace `YOUR_PUBLIC_HOST` with the hostname from Step 8 and run the command.

> **Why register via API rather than in the developer portal?** The developer portal only shows a sandbox environment. Production webhooks must be registered via the API against your live account.

> **What if I change my public URL?** The old webhook will still be registered and pointing at a dead URL. Delete it first (`DELETE https://api.monzo.com/webhooks/{id}`) and register the new one. The diagnostics tool lists any existing webhooks.

---

## Step 10 — Verify everything

```bash
./target/release/monzoctl diagnose
```

A fully working setup looks like this:

```
=== monzoboiii diagnostics ===

[OK]   Config loaded from /home/ben/.config/monzoboiii/config.toml
[OK]   Tokens found
[OK]   Authenticated (user_id: user_0000AbCdEf)
[OK]   account_id (acc_0000AbCdEfGhIjKlMn01) confirmed
[OK]   2 spending pot(s) found: groceries (pot_xxx), transport (pot_yyy)
[OK]   Webhook registered: https://your-host/webhook/monzo/your-secret

Diagnostics complete.
```

Make a small card payment in a category you have a pot for. The server should log something like:

```
INFO monzoboiii: Pot withdrawal triggered for category 'groceries' tx tx_0000AbCd...
```

---

## Appendix A — Running as a systemd service

The Pi doesn't hold the source checkout or a Rust toolchain — it only has compiled binaries and `tokens.toml` in `/home/ben/monzoboiii-run/`. The systemd unit (`deploy/monzoboiii.service` in this repo) is built, deployed, and managed remotely from your dev machine. See **[DEV_WORKFLOW.md](DEV_WORKFLOW.md)** for the full cross-compile/deploy/tunnel setup — the short version is `just reload`.

---

## Appendix B — Re-authenticating after token expiry

Monzo refresh tokens can be revoked if you log out of all devices, change your password, or if the token goes unused for a long time. If that happens, the server will start logging `401 Unauthorized` errors and withdrawals will stop.

Re-authentication is the same as Step 5 — visit `/auth/reauth` in a browser. You do not need to re-register the webhook or recreate pots.

---

## Appendix C — Quick reference

| Thing to check | How |
|---|---|
| Server logs | `journalctl -u monzoboiii -f` (if using systemd) |
| Token validity | `./target/release/monzoctl diagnose` |
| Pot map (updated every 3h) | Server logs show count on each refresh |
| Webhook deliveries | Monzo developer portal → your client → Webhooks |
| Delete a webhook | `curl -X DELETE https://api.monzo.com/webhooks/{id} -H 'Authorization: Bearer TOKEN'` |
