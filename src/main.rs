//! pg: Paragma, memory for coding agents.

mod agents;
mod cli;
mod code;
mod db;
mod embed;
mod index;
mod libs;
mod note;
mod output;
mod project;
mod record;
mod search;
mod skills;
mod state;
mod sync;
mod terminal;
mod testrun;
mod upgrade;

fn main() -> std::process::ExitCode {
    output::mark("start");
    quiet_when_output_closes();
    cli::run()
}

/// An agent that pipes pg into `head` closes the output early: pg stops
/// there without a panic message (its work is done before it prints).
fn quiet_when_output_closes() {
    let default = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let text = info
            .payload()
            .downcast_ref::<String>()
            .map(String::as_str)
            .or_else(|| info.payload().downcast_ref::<&str>().copied())
            .unwrap_or("");
        if text.starts_with("failed printing to stdout") {
            std::process::exit(0);
        }
        default(info);
    }));
}
