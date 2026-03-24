use clap::Parser;

mod common;
mod dcui;
mod installer;

#[derive(Parser)]
#[command(name = "vmm-appliance", about = "CoreVM Appliance Installer & DCUI")]
struct Cli {
    #[arg(long, value_enum)]
    mode: Mode,
}

#[derive(Clone, clap::ValueEnum)]
enum Mode {
    Installer,
    Dcui,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.mode {
        Mode::Installer => installer::run()?,
        Mode::Dcui => dcui::run()?,
    }
    Ok(())
}
