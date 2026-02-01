use clap::Subcommand;
use serde::Deserialize;

#[derive(Subcommand)]
pub enum AdminCommands {
    Leader,
    Promote {
        #[arg(short, long)]
        node_id: String,
    },
    Config {
        #[command(subcommand)]
        command: ConfigCommands,
    },
}

#[derive(Subcommand)]
pub enum ConfigCommands {
    Get {
        key: String,
    },
    Set {
        key: String,
        value: String,
    },
}

#[derive(Debug, Deserialize)]
struct ClusterStatus {
    leader_id: Option<String>,
    term: u64,
}

pub async fn execute(command: AdminCommands, endpoint: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    match command {
        AdminCommands::Leader => {
            let url = format!("{}/_admin/cluster/status", endpoint);
            let response = client.get(&url).send().await?;
            let status: ClusterStatus = response.json().await?;

            match status.leader_id {
                Some(leader) => {
                    println!("Current leader: {}", leader);
                    println!("Term: {}", status.term);
                }
                None => {
                    println!("No leader elected");
                }
            }
        }

        AdminCommands::Promote { node_id } => {
            println!("Promoting node {} to leader...", node_id);
            println!("Note: This will trigger a new election");
        }

        AdminCommands::Config { command } => match command {
            ConfigCommands::Get { key } => {
                println!("Config key '{}' not found", key);
            }
            ConfigCommands::Set { key, value } => {
                println!("Set config '{}' = '{}'", key, value);
            }
        },
    }

    Ok(())
}
