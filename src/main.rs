use std::env;
use clap::Parser;
use serde_json::to_string_pretty;

mod cmd;
mod api;
mod ssh;
mod service_env;
mod service_script;

#[derive(Debug, Parser)]
struct Args {
    #[command(subcommand)]
    cmd: cmd::Cmd,
}

#[tokio::main]
async fn main() {
    if env::var(env_logger::DEFAULT_FILTER_ENV).is_ok() {
        env_logger::init();
    } else {
        env_logger::builder().filter_level(log::LevelFilter::Info).init();
    }

    let args = Args::parse();
    match args.cmd.run().await {
        Ok(_) => {},
        Err(e) => {
            // Print the error message as json, so as to show what happens in API
            log::error!("Uncaught Error: {}", to_string_pretty(&e).unwrap());
        }
    }
}

