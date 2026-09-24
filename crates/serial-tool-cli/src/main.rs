use clap::Parser;
use serial_tool_cli::Cli;

fn main() -> std::process::ExitCode {
    std::process::ExitCode::from(Cli::parse().run() as u8)
}
