use clap::Parser;
use serde_json::to_string_pretty;

mod cmd;
mod api;
mod service_env;

#[derive(Debug, Parser)]
struct Args {
    #[command(subcommand)]
    cmd: cmd::Cmd,
}

#[tokio::main]
async fn main() {
    env_logger::init();
    let args = Args::parse();
    match args.cmd.run().await {
        Ok(_) => {},
        Err(e) => {
            // Print the error message as json, so as to show what happens in API
            eprintln!("Uncaught Error: {}", to_string_pretty(&e).unwrap());
        }
    }
}

