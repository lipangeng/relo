use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "relo", version)]
pub struct Cli {
    #[arg(short = 'd', long = "dir", default_value = ".")]
    pub dir: PathBuf,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Init {
        #[arg(value_enum)]
        target: Option<InitTarget>,
        #[arg(long, value_enum)]
        home: Option<HomeArg>,
        #[arg(long = "path")]
        path_prepend: Vec<String>,
        #[arg(long = "path-append")]
        path_append: Vec<String>,
    },
    List,
    Show {
        version: Option<String>,
    },
    Use {
        #[arg(short = 'g', long = "global")]
        global: bool,
        #[arg(long, value_enum)]
        shell: Option<ShellArg>,
        #[arg(long = "path")]
        path_prepend: Vec<String>,
        #[arg(long = "path-append")]
        path_append: Vec<String>,
        version: Option<String>,
    },
    Print {
        #[arg(value_enum)]
        target: PrintTarget,
        version: Option<String>,
    },
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
}

#[derive(Clone, ValueEnum)]
pub enum InitTarget {
    Zsh,
    Bash,
    Powershell,
    Cmd,
}

#[derive(Clone, ValueEnum)]
pub enum HomeArg {
    Shared,
    Versioned,
}

#[derive(Clone, ValueEnum)]
pub enum PrintTarget {
    Root,
    Active,
    Release,
    Home,
    Version,
}

#[derive(Clone, Subcommand)]
pub enum ConfigCommand {
    Show,
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Posix,
    Powershell,
    Cmd,
}
