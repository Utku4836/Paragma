//! pg: Paragma, memory for coding agents.

mod agents;
mod cli;
mod code;
mod db;
mod embed;
mod index;
mod note;
mod output;
mod project;
mod record;
mod search;
mod skills;
mod state;
mod terminal;
mod testrun;
mod upgrade;

fn main() -> std::process::ExitCode {
    output::mark("start");
    cli::run()
}
