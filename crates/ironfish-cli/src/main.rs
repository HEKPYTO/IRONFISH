use clap::{Parser, Subcommand};
use ironfish_cli::commands::{admin, cluster, node, token};
#[derive(Parser)]
#[command(name = "ironfish")]
#[command(author, version, about = "Ironfish Chess Analysis CLI", long_about = None)]
struct Cli {
    #[arg(short, long, default_value = "http://localhost:8080")]
    endpoint: String,
    #[command(subcommand)]
    command: Commands,
}
#[derive(Subcommand)]
enum Commands {
    Cluster {
        #[command(subcommand)]
        command: cluster::ClusterCommands,
    },
    Node {
        #[command(subcommand)]
        command: node::NodeCommands,
    },
    Token {
        #[command(subcommand)]
        command: token::TokenCommands,
    },
    Admin {
        #[command(subcommand)]
        command: admin::AdminCommands,
    },
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Cluster { command } => cluster::execute(command, &cli.endpoint).await?,
        Commands::Node { command } => node::execute(command, &cli.endpoint).await?,
        Commands::Token { command } => token::execute(command, &cli.endpoint).await?,
        Commands::Admin { command } => admin::execute(command, &cli.endpoint).await?,
    }
    Ok(())
}
