use monzoboiii::config::{Config, Tokens};
use reqwest::Client;
use std::path::Path;

const CONFIG_PATH: &str = "/home/ben/.config/monzoboiii/config.toml";
const TOKENS_PATH: &str = "tokens.toml";

const SPENDING_CATEGORIES: &[&str] = &[
    "general",
    "eating_out",
    "expenses",
    "transport",
    "cash",
    // "bills",
    "entertainment",
    "shopping",
    "holidays",
    "groceries",
    "family",
    "charity",
    "personal_care",
    // "savings",
];

#[tokio::main]
async fn main() {
    println!("=== monzoboiii diagnostics ===\n");

    // --- Config ---
    let config = match Config::load(Path::new(CONFIG_PATH)) {
        Ok(c) => {
            ok(&format!("Config loaded from {CONFIG_PATH}"));
            c
        }
        Err(e) => {
            fail(&format!("Config: {e}"));
            hint(&format!(
                "Copy config.toml.example to {CONFIG_PATH} and fill it in"
            ));
            return;
        }
    };

    // --- Tokens ---
    let tokens = Tokens::load(Path::new(TOKENS_PATH)).unwrap_or_default();
    if tokens.access_token.is_empty() {
        warn("No tokens — not yet authenticated");
        hint(&format!(
            "Start the server and visit http://localhost:{}/auth/reauth",
            config.app.port
        ));
        println!("\nCannot check the Monzo API without tokens. Authenticate first.");
        return;
    }
    ok("Tokens found");

    let http = Client::new();

    // --- Reachability + auth ---
    match http
        .get("https://api.monzo.com/ping/whoami")
        .bearer_auth(&tokens.access_token)
        .send()
        .await
    {
        Err(e) => {
            fail(&format!("Cannot reach Monzo API: {e}"));
            return;
        }
        Ok(res) if res.status() == 401 => {
            fail("Auth token is invalid or expired");
            hint(&format!(
                "Visit http://localhost:{}/auth/reauth to re-authenticate",
                config.app.port
            ));
            return;
        }
        Ok(res) if !res.status().is_success() => {
            fail(&format!(
                "Monzo API returned unexpected status {}",
                res.status()
            ));
            return;
        }
        Ok(res) => {
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            let user_id = body["user_id"].as_str().unwrap_or("unknown");
            ok(&format!("Authenticated (user_id: {user_id})"));
        }
    }

    // --- Account ID ---
    let accounts_ok = match http
        .get("https://api.monzo.com/accounts")
        .bearer_auth(&tokens.access_token)
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            let ids: Vec<&str> = body["accounts"]
                .as_array()
                .map(|arr| arr.iter().filter_map(|a| a["id"].as_str()).collect())
                .unwrap_or_default();

            let configured = &config.monzo.account_id;
            if ids.contains(&configured.as_str()) {
                ok(&format!("account_id ({configured}) confirmed"));
                true
            } else {
                fail(&format!(
                    "Configured account_id ({configured}) not found in your accounts"
                ));
                if ids.is_empty() {
                    hint("No accounts returned — you may need to re-authenticate");
                } else {
                    hint(&format!("Your account IDs: {}", ids.join(", ")));
                    hint(&format!(
                        "Update account_id in {CONFIG_PATH} and restart the server"
                    ));
                }
                false
            }
        }
        Ok(res) => {
            warn(&format!("Could not list accounts ({})", res.status()));
            false
        }
        Err(e) => {
            warn(&format!("Could not reach accounts endpoint: {e}"));
            false
        }
    };

    if !accounts_ok {
        println!("\nFix account_id before checking pots and webhooks.");
        return;
    }

    // --- Pots ---
    match http
        .get("https://api.monzo.com/pots")
        .bearer_auth(&tokens.access_token)
        .query(&[("current_account_id", config.monzo.account_id.as_str())])
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            let pots = body["pots"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let matching: Vec<String> = pots
                .iter()
                .filter(|p| p["deleted"].as_bool() != Some(true))
                .filter_map(|p| {
                    let name = p["name"].as_str()?.replace(" ", "_").to_lowercase();
                    let id = p["id"].as_str()?;
                    SPENDING_CATEGORIES
                        .contains(&name.as_str())
                        .then(|| format!("{name} ({id})"))
                })
                .collect();

            if matching.is_empty() {
                warn("No spending-category pots found");
                hint("Create pots in the Monzo app with names from this list:");
                hint(&SPENDING_CATEGORIES.join(", "));
            } else {
                ok(&format!(
                    "{} spending pot(s) found: {}",
                    matching.len(),
                    matching.join(", ")
                ));
            }
        }
        Ok(res) => warn(&format!("Could not list pots ({})", res.status())),
        Err(e) => warn(&format!("Could not reach pots endpoint: {e}")),
    }

    // --- Webhooks ---
    match http
        .get("https://api.monzo.com/webhooks")
        .bearer_auth(&tokens.access_token)
        .query(&[("account_id", config.monzo.account_id.as_str())])
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => {
            let body: serde_json::Value = res.json().await.unwrap_or_default();
            let webhooks = body["webhooks"]
                .as_array()
                .map(Vec::as_slice)
                .unwrap_or_default();
            let expected_suffix = format!("/webhook/monzo/{}", config.app.secret);
            let matched: Vec<&str> = webhooks
                .iter()
                .filter_map(|w| w["url"].as_str())
                .filter(|url| url.ends_with(&expected_suffix))
                .collect();

            if matched.is_empty() {
                warn("No webhook registered matching your secret");
                hint("Register one with this curl command (replace YOUR_PUBLIC_HOST):");
                println!(
                    "\n  curl -X POST https://api.monzo.com/webhooks \\\n    \
                     -H 'Authorization: Bearer {}' \\\n    \
                     -d 'account_id={}' \\\n    \
                     -d 'url=https://YOUR_PUBLIC_HOST/webhook/monzo/{}'\n",
                    tokens.access_token, config.monzo.account_id, config.app.secret
                );
                if !webhooks.is_empty() {
                    println!("  Existing webhooks on this account:");
                    for w in webhooks {
                        println!("    {}", w["url"].as_str().unwrap_or("?"));
                    }
                    println!();
                }
            } else {
                ok(&format!("Webhook registered: {}", matched[0]));
            }
        }
        Ok(res) => warn(&format!("Could not list webhooks ({})", res.status())),
        Err(e) => warn(&format!("Could not reach webhooks endpoint: {e}")),
    }

    println!("\nDiagnostics complete.");
}

fn ok(msg: &str) {
    println!("[OK]   {msg}");
}
fn warn(msg: &str) {
    println!("[WARN] {msg}");
}
fn fail(msg: &str) {
    println!("[FAIL] {msg}");
}
fn hint(msg: &str) {
    println!("         → {msg}");
}
