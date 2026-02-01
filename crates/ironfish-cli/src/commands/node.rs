use clap::Subcommand;
use serde::Deserialize;

#[derive(Subcommand)]
pub enum NodeCommands {
    Info,
    Health,
    Metrics,
}

#[derive(Debug, Deserialize)]
struct HealthResponse {
    status: String,
    node_id: String,
    version: String,
}

#[derive(Debug, Deserialize)]
struct MetricsResponse {
    cpu_usage: f32,
    memory_usage: f32,
    active_analyses: u32,
    queue_depth: u32,
    engines_available: u32,
    engines_total: u32,
}

pub async fn execute(command: NodeCommands, endpoint: &str) -> anyhow::Result<()> {
    let client = reqwest::Client::new();

    match command {
        NodeCommands::Info => {
            let url = format!("{}/health", endpoint);
            let response = client.get(&url).send().await?;
            let health: HealthResponse = response.json().await?;

            println!("Node Information:");
            println!("  ID: {}", health.node_id);
            println!("  Version: {}", health.version);
            println!("  Status: {}", health.status);
        }

        NodeCommands::Health => {
            let url = format!("{}/health", endpoint);
            let response = client.get(&url).send().await?;
            let health: HealthResponse = response.json().await?;

            if health.status == "healthy" {
                println!("Node is healthy");
            } else {
                println!("Node is unhealthy: {}", health.status);
            }
        }

        NodeCommands::Metrics => {
            let url = format!("{}/metrics", endpoint);
            let response = client.get(&url).send().await?;
            let metrics: MetricsResponse = response.json().await?;

            println!("Node Metrics:");
            println!("  CPU Usage: {:.1}%", metrics.cpu_usage * 100.0);
            println!("  Memory Usage: {:.1}%", metrics.memory_usage * 100.0);
            println!("  Active Analyses: {}", metrics.active_analyses);
            println!("  Queue Depth: {}", metrics.queue_depth);
            println!(
                "  Engines: {}/{}",
                metrics.engines_available, metrics.engines_total
            );
        }
    }

    Ok(())
}
