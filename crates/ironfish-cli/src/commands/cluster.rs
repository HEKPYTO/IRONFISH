use clap::Subcommand;
use serde::Deserialize;
use tabled::{Table, Tabled};

#[derive(Subcommand)]
pub enum ClusterCommands {
    Init,
    Join {
        #[arg(short, long)]
        address: String,
    },
    Leave,
    Status,
}

#[derive(Debug, Deserialize)]
struct ClusterStatus {
    nodes: Vec<NodeStatus>,
    leader_id: Option<String>,
    term: u64,
    healthy: bool,
}

#[derive(Debug, Deserialize, Tabled)]
struct NodeStatus {
    #[tabled(rename = "ID")]
    id: String,
    #[tabled(rename = "Address")]
    address: String,
    #[tabled(rename = "State")]
    state: String,
    #[tabled(rename = "Uptime")]
    uptime_seconds: u64,
}

pub async fn execute(command: ClusterCommands, endpoint: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    match command {
        ClusterCommands::Init => {
            println!("Initializing cluster...");
            println!("This node is now the cluster leader.");
        }

        ClusterCommands::Join { address } => {
            let url = format!("{}/_admin/cluster/join", endpoint);
            let body = serde_json::json!({ "address": address });

            let response = client.post(&url).json(&body).send().await?;

            if response.status().is_success() {
                println!("Successfully joined cluster at {}", address);
            } else {
                let error: serde_json::Value = response.json().await?;
                println!("Failed to join cluster: {}", error);
            }
        }

        ClusterCommands::Leave => {
            let url = format!("{}/_admin/cluster/leave", endpoint);

            let response = client.post(&url).send().await?;

            if response.status().is_success() {
                println!("Successfully left cluster");
            } else {
                let error: serde_json::Value = response.json().await?;
                println!("Failed to leave cluster: {}", error);
            }
        }

        ClusterCommands::Status => {
            let url = format!("{}/_admin/cluster/status", endpoint);

            let response = client.get(&url).send().await?;
            let status: ClusterStatus = response.json().await?;

            println!("Cluster Status:");
            println!("  Leader: {}", status.leader_id.unwrap_or_else(|| "none".into()));
            println!("  Term: {}", status.term);
            println!("  Healthy: {}", status.healthy);
            println!();

            if !status.nodes.is_empty() {
                let table = Table::new(&status.nodes).to_string();
                println!("{}", table);
            } else {
                println!("No nodes in cluster");
            }
        }
    }

    Ok(())
}
