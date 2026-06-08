mod cli;
mod config;
mod error;
mod layout;
mod shell;
mod version;

use anyhow::Result;
use clap::{CommandFactory, Parser};
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
    let root = context_dir(&cli);

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
                    println!("context:  {}", layout.root.display());
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
            path_append,
            verbose,
            help,
            version_flag,
            version,
        } => {
            if help {
                if let Some(shell) = shell {
                    print!(
                        "{}",
                        shell::forward(&["use", "--help"], ShellKind::from(shell))?
                    );
                } else {
                    print_use_help();
                }
                return Ok(());
            }
            if version_flag {
                if let Some(shell) = shell {
                    print!(
                        "{}",
                        shell::forward(&["use", "--version"], ShellKind::from(shell))?
                    );
                } else {
                    println!("relo {}", env!("CARGO_PKG_VERSION"));
                }
                return Ok(());
            }
            if let (true, Some(shell)) = (global, shell) {
                print!(
                    "{}",
                    shell::forward(
                        &use_forward_args(global, verbose, version.as_deref()),
                        ShellKind::from(shell)
                    )?
                );
                return Ok(());
            }
            let layout = Layout::load(root)?;
            if global && !path.is_empty() {
                anyhow::bail!("path overrides are only valid for local use");
            }
            let release = match version {
                Some(expr) => layout.resolve(&expr)?,
                None => layout.default_release()?,
            };
            if verbose > 0 {
                print_use_verbose(&layout, &release.id, global, &path)?;
            }
            if global {
                layout.set_active(&release.id)?;
            } else {
                layout.ensure_home(&release.id)?;
                let shell = shell
                    .map(ShellKind::from)
                    .unwrap_or_else(ShellKind::default_for_platform);
                print!(
                    "{}",
                    shell::exports(&layout, &release.id, shell, &path, path_append)?
                );
            }
        }
        Command::Print { target, version } => {
            let layout = Layout::load(root)?;
            match target {
                PrintTarget::Context | PrintTarget::Ctx => {
                    reject_print_version(&target, &version)?;
                    println!("{}", layout.root.display());
                }
                PrintTarget::Active => {
                    reject_print_version(&target, &version)?;
                    println!("{}", layout.active_path().display());
                }
                PrintTarget::Version => {
                    let release = print_release(&layout, version.as_deref())?;
                    println!("{}", release.id);
                }
                PrintTarget::Release => {
                    let release = print_release(&layout, version.as_deref())?;
                    println!("{}", release.path.display());
                }
                PrintTarget::Home => {
                    let release = print_release(&layout, version.as_deref())?;
                    println!("{}", layout.home_for(&release.id).display());
                }
                PrintTarget::Path => {
                    let release = print_release(&layout, version.as_deref())?;
                    for path in layout.effective_path(&release.id, &[])? {
                        println!("{}", path.display());
                    }
                }
                PrintTarget::Env => {
                    let release = print_release(&layout, version.as_deref())?;
                    for (name, value) in layout.effective_env(&release.id)? {
                        println!("{name}={value}");
                    }
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

fn print_use_help() {
    let mut command = Cli::command();
    let use_command = command
        .find_subcommand_mut("use")
        .expect("use subcommand exists");
    print!("{}", use_command.render_long_help());
}

fn print_release(layout: &Layout, version: Option<&str>) -> Result<crate::version::Release> {
    match version {
        Some(expr) => layout.resolve(expr),
        None => layout.default_release(),
    }
}

fn print_use_verbose(
    layout: &Layout,
    release_id: &str,
    global: bool,
    path: &[String],
) -> Result<()> {
    let release = layout.resolve(release_id)?;
    eprintln!("version: {}", release.id);
    eprintln!("release: {}", release.path.display());
    eprintln!("mode: {}", if global { "global" } else { "local" });

    eprintln!("env:");
    let env = layout.effective_env(&release.id)?;
    if env.is_empty() {
        eprintln!("  (none)");
    } else {
        for (name, value) in env {
            eprintln!("  {name}={value}");
        }
    }

    eprintln!("path:");
    let path = if global {
        layout.effective_path(&release.id, &[])?
    } else {
        layout.effective_path(&release.id, path)?
    };
    if path.is_empty() {
        eprintln!("  (none)");
    } else {
        for path in path {
            eprintln!("  {}", path.display());
        }
    }

    Ok(())
}

fn reject_print_version(target: &PrintTarget, version: &Option<String>) -> Result<()> {
    if version.is_some() {
        anyhow::bail!("--version is not valid for print {}", target.as_str());
    }
    Ok(())
}

fn use_forward_args(global: bool, verbose: u8, version: Option<&str>) -> Vec<String> {
    let mut args = vec!["use".to_string()];
    if global {
        args.push("--global".to_string());
    }
    for _ in 0..verbose {
        args.push("-v".to_string());
    }
    if let Some(version) = version {
        args.push(version.to_string());
    }
    args
}

fn context_dir(cli: &Cli) -> PathBuf {
    let dir = cli
        .dir
        .clone()
        .or_else(|| {
            std::env::var_os("RELO_CONTEXT")
                .filter(|value| !value.is_empty())
                .map(PathBuf::from)
        })
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
