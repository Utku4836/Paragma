//! pg: Paragma, memory for coding agents.

mod agents;
mod cli;
mod code;
mod embed;
mod index;
mod note;
mod output;
mod project;
mod record;
mod search;
mod skills;
mod state;
mod testrun;

fn main() -> std::process::ExitCode {
    cli::run()
}
