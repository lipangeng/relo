use clap::{Parser, Subcommand, ValueEnum};
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "relo", version)]
pub struct Cli {
    #[arg(short = 'd', long = "dir")]
    pub dir: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    Init {
        #[arg(value_enum)]
        target: Option<InitTarget>,
        #[arg(long)]
        force: bool,
        #[arg(long, value_enum)]
        home: Option<HomeArg>,
        #[arg(long = "path")]
        path: Vec<String>,
    },
    List,
    Show {
        version: Option<String>,
    },
    #[command(disable_help_flag = true)]
    Use {
        #[arg(short = 'g', long = "global")]
        global: bool,
        #[arg(long, value_enum)]
        shell: Option<ShellArg>,
        #[arg(long = "path")]
        path: Vec<String>,
        #[arg(long = "path-append")]
        path_append: bool,
        #[arg(short = 'v', long = "verbose", action = clap::ArgAction::Count)]
        verbose: u8,
        #[arg(short = 'h', long = "help")]
        help: bool,
        #[arg(short = 'V', long = "version")]
        version_flag: bool,
        version: Option<String>,
    },
    Print {
        #[arg(value_enum)]
        target: PrintTarget,
        #[arg(long = "version")]
        version: Option<String>,
    },
    Config {
        #[command(subcommand)]
        command: Option<ConfigCommand>,
    },
    /// macOS-specific release helpers
    Mac {
        #[command(subcommand)]
        command: MacCommand,
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
    Context,
    Ctx,
    Active,
    Release,
    Home,
    Version,
    Path,
    Env,
}

impl PrintTarget {
    pub fn as_str(&self) -> &'static str {
        match self {
            PrintTarget::Context => "context",
            PrintTarget::Ctx => "ctx",
            PrintTarget::Active => "active",
            PrintTarget::Release => "release",
            PrintTarget::Home => "home",
            PrintTarget::Version => "version",
            PrintTarget::Path => "path",
            PrintTarget::Env => "env",
        }
    }
}

#[derive(Clone, Subcommand)]
pub enum ConfigCommand {
    Show,
}

#[derive(Clone, Subcommand)]
pub enum MacCommand {
    /// Remove the macOS quarantine attribute from a release
    Unblock {
        #[arg(
            short = 'v',
            long = "verbose",
            action = clap::ArgAction::Count,
            help = "Show release and quarantine details"
        )]
        verbose: u8,
        version: Option<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum ShellArg {
    Posix,
    Powershell,
    Cmd,
}
