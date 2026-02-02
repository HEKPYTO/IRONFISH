use clap::Subcommand;
use serde::Deserialize;
use tabled::{Table, Tabled};
#[derive(Subcommand)]
pub enum TokenCommands {
    Create {
        #[arg(short, long)]
        name: Option<String>,
        #[arg(short, long)]
        expires_in_days: Option<u32>,
    },
    Revoke {
        #[arg(short, long)]
        id: String,
    },
    List,
}
#[derive(Debug, Deserialize)]
struct CreateTokenResponse {
    id: String,
    token: String,
    expires_at: Option<String>,
}
#[derive(Debug, Deserialize, Tabled)]
struct TokenInfo {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Name", display_with = "display_option")]
    name: Option<String>,
    #[tabled(rename = "Created")]
    created_at: String,
    #[tabled(rename = "Expires", display_with = "display_option")]
    expires_at: Option<String>,
    #[tabled(rename = "Revoked")]
    revoked: bool,
}
fn display_option(o: &Option<String>) -> String {
    o.clone().unwrap_or_else(|| "-".to_string())
}
pub async fn execute(command: TokenCommands, endpoint: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();
    match command {
        TokenCommands::Create {
            name,
            expires_in_days,
        } => {
            let url = format!("{}/_admin/tokens", endpoint);
            let body = serde_json::json!({
                "name": name,
                "expires_in_days": expires_in_days,
            });
            let response = client.post(&url).json(&body).send().await?;
            if response.status().is_success() {
                let token: CreateTokenResponse = response.json().await?;
                println!("Token created successfully!");
                println!();
                println!("  ID: {}", token.id);
                println!("  Token: {}", token.token);
                if let Some(expires) = token.expires_at {
                    println!("  Expires: {}", expires);
                }
                println!();
                println!("Save this token - it won't be shown again!");
            } else {
                let error: serde_json::Value = response.json().await?;
                println!("Failed to create token: {}", error);
            }
        }
        TokenCommands::Revoke { id } => {
            let url = format!("{}/_admin/tokens/{}", endpoint, id);
            let response = client.delete(&url).send().await?;
            if response.status().is_success() {
                println!("Token {} revoked successfully", id);
            } else {
                let error: serde_json::Value = response.json().await?;
                println!("Failed to revoke token: {}", error);
            }
        }
        TokenCommands::List => {
            let url = format!("{}/_admin/tokens", endpoint);
            let response = client.get(&url).send().await?;
            let tokens: Vec<TokenInfo> = response.json().await?;
            if tokens.is_empty() {
                println!("No tokens found");
            } else {
                let table = Table::new(&tokens).to_string();
                println!("{}", table);
            }
        }
    }
    Ok(())
}
