use clap::Parser;

mod cmd;
mod api;
mod service_env;

#[derive(Debug, Parser)]
struct Args {
    #[command(subcommand)]
    cmd: cmd::Cmd,
}

#[tokio::main]
async fn main() -> Result<(), cmd::Error> {
    env_logger::init();
    let args = Args::parse();
    args.cmd.run().await
}

