//! `dasho` — the command-line harness around the Dasho transfer engine.

use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(
    name = "dasho",
    version,
    about = "Files that move. Fast, verifiable, restartable transfers between your devices."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Run in receive mode: accept transfers from other devices into a target folder
    Serve,
    /// Send files or directories to a nearby device
    Send,
    /// List Dasho devices discovered on the local network
    Devices,
    /// Run benchmark scenarios against the engine
    Bench,
}

fn main() {
    let cli = Cli::parse();
    match cli.command {
        Command::Serve | Command::Send | Command::Devices | Command::Bench => {
            eprintln!(
                "error: `dasho {}` is not wired to the engine yet — it lands with the v1 engine milestone",
                match cli.command {
                    Command::Serve => "serve",
                    Command::Send => "send",
                    Command::Devices => "devices",
                    Command::Bench => "bench",
                }
            );
            std::process::exit(2);
        }
    }
}
