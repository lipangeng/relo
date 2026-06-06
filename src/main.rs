mod cli;
mod config;
mod error;
mod layout;
mod shell;
mod version;

use anyhow::Result;
use clap::Parser;
use cli::{Cli, Command, ConfigCommand, HomeArg, InitTarget, PrintTarget, ShellArg};
use layout::Layout;
use shell::ShellKind;
use std::path::PathBuf;

fn main() {
    if let Err(err) = run() {
        eprintln!("{err}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let root = root_dir(&cli);

    match cli.command {
        Command::Init {
            target,
            force,
            home,
            path,
        } => match target {
            Some(InitTarget::Zsh) | Some(InitTarget::Bash) => {
                print!("{}", shell::wrapper(ShellKind::Posix));
            }
            Some(InitTarget::Powershell) => {
                print!("{}", shell::wrapper(ShellKind::Powershell));
            }
            Some(InitTarget::Cmd) => {
                print!("{}", shell::wrapper(ShellKind::Cmd));
            }
            None => {
                let mode = home.unwrap_or(HomeArg::Shared).into();
                Layout::init(&root, mode, path, force)?;
            }
        },
        Command::List => {
            let layout = Layout::load(root)?;
            let active = layout.active_version()?;
            let (releases, invalid) = layout.releases_with_invalid()?;
            for release in releases {
                let marker = if Some(release.id.as_str()) == active.as_deref() {
                    '*'
                } else {
                    ' '
                };
                println!("{marker} {}", release.id);
            }
            for id in invalid {
                println!("! {id} (invalid)");
            }
        }
        Command::Show { version } => {
            let layout = Layout::load(root)?;
            match version {
                Some(expr) => {
                    let release = layout.resolve(&expr)?;
                    let active = layout.active_version()?.as_deref() == Some(release.id.as_str());
                    println!("version:  {}", release.id);
                    println!("release:  {}", release.path.display());
                    println!("home:     {}", layout.home_for(&release.id).display());
                    println!("active:   {}", if active { "yes" } else { "no" });
                }
                None => {
                    let active = layout.active_version()?;
                    println!("root:     {}", layout.root.display());
                    println!("active:   {}", active.as_deref().unwrap_or("none"));
                    if let Some(active) = active.as_deref() {
                        println!("release:  {}", layout.release_path(active).display());
                        println!("home:     {}", layout.home_for(active).display());
                    } else {
                        println!("release:  none");
                        println!("home:     none");
                    }
                    println!("mode:     {}", layout.config.home_mode.as_str());
                    println!("releases: {}", layout.releases()?.len());
                }
            }
        }
        Command::Use {
            global,
            shell,
            path,
            version,
        } => {
            let layout = Layout::load(root)?;
            if global && version.is_none() {
                anyhow::bail!("global use requires a version");
            }
            if global && !path.is_empty() {
                anyhow::bail!("path overrides are only valid for local use");
            }
            let release = match version {
                Some(expr) => layout.resolve(&expr)?,
                None => layout.active_release()?,
            };
            if global {
                layout.set_active(&release.id)?;
            } else {
                layout.ensure_home(&release.id)?;
                let shell = shell
                    .map(ShellKind::from)
                    .unwrap_or_else(ShellKind::default_for_platform);
                print!("{}", shell::exports(&layout, &release.id, shell, &path)?);
            }
        }
        Command::Print { target, version } => {
            let layout = Layout::load(root)?;
            match target {
                PrintTarget::Root => println!("{}", layout.root.display()),
                PrintTarget::Active => println!("{}", layout.active_path().display()),
                PrintTarget::Version => {
                    let release = match version {
                        Some(expr) => layout.resolve(&expr)?,
                        None => layout.active_release()?,
                    };
                    println!("{}", release.id);
                }
                PrintTarget::Release => {
                    let release = match version {
                        Some(expr) => layout.resolve(&expr)?,
                        None => layout.active_release()?,
                    };
                    println!("{}", release.path.display());
                }
                PrintTarget::Home => {
                    let release = match version {
                        Some(expr) => layout.resolve(&expr)?,
                        None => layout.active_release()?,
                    };
                    println!("{}", layout.home_for(&release.id).display());
                }
            }
        }
        Command::Config { command } => match command.unwrap_or(ConfigCommand::Show) {
            ConfigCommand::Show => {
                let layout = Layout::load(root)?;
                print!("{}", std::fs::read_to_string(layout.config_path())?);
            }
        },
    }

    Ok(())
}

fn root_dir(cli: &Cli) -> PathBuf {
    let dir = cli
        .dir
        .clone()
        .or_else(|| {
            std::env::var_os("RELO_CTX")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
        .unwrap_or_else(|| PathBuf::from("."));
    std::fs::canonicalize(&dir).unwrap_or(dir)
}

impl From<ShellArg> for ShellKind {
    fn from(value: ShellArg) -> Self {
        match value {
            ShellArg::Posix => ShellKind::Posix,
            ShellArg::Powershell => ShellKind::Powershell,
            ShellArg::Cmd => ShellKind::Cmd,
        }
    }
}
