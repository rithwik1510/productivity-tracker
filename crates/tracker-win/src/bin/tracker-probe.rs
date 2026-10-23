//! Headless probe for the Windows adapters, used by the ADR-0002 spike harness and for checking
//! the catalog by hand. Prints JSON on stdout, logs on stderr.

use std::io::Write;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Context;
use clap::{Parser, Subcommand};
use tracker_api::Sampler;
use tracker_win::spike::{self, Watch};
use tracker_win::{SessionWatcher, WinSampler, catalog};

#[derive(Parser)]
#[command(about = "Probe the Windows adapters without the app")]
struct Cli {
    /// Log debug output to stderr.
    #[arg(long, short, global = true)]
    verbose: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Print one JSON line (a `Sample`) per tick: identity, browser view, title, idle, audible, away.
    Probe {
        /// Read this process's topmost window instead of the foreground window.
        #[arg(long, conflicts_with = "hwnd")]
        pid: Option<u32>,
        /// Read this window handle (decimal or 0x-prefixed hex).
        #[arg(long, value_parser = parse_hwnd)]
        hwnd: Option<isize>,
        #[arg(long, default_value_t = 1000)]
        interval_ms: u64,
    },
    /// Drive a browser for the spike.
    #[command(subcommand)]
    Spike(Spike),
    /// Print the app catalog (installed and running apps) as JSON, writing icons as the app would.
    /// The JSON lists the icons already on disk; the probe then waits for the rest to be written.
    Catalog {
        /// Data dir for the icon cache; defaults to the app's own data dir.
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
}

#[derive(Subcommand)]
enum Spike {
    /// Type draft text into the omnibox via UIA SetFocus + SetValue.
    SetOmnibox {
        #[arg(long)]
        pid: u32,
        #[arg(long)]
        text: String,
    },
    /// Bring that browser's window to the front.
    Focus {
        #[arg(long)]
        pid: u32,
    },
}

fn parse_hwnd(text: &str) -> Result<isize, std::num::ParseIntError> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => isize::from_str_radix(hex, 16),
        None => text.parse(),
    }
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(if cli.verbose {
            tracing::Level::DEBUG
        } else {
            tracing::Level::INFO
        })
        .init();
    match cli.command {
        Command::Probe {
            pid,
            hwnd,
            interval_ms,
        } => {
            let watch = match (pid, hwnd) {
                (Some(pid), _) => Watch::Pid(pid),
                (None, Some(hwnd)) => Watch::Hwnd(hwnd),
                (None, None) => Watch::Foreground,
            };
            probe(watch, Duration::from_millis(interval_ms.max(1)))
        }
        Command::Spike(Spike::SetOmnibox { pid, text }) => {
            spike::set_omnibox(pid, &text).context("set-omnibox")
        }
        Command::Spike(Spike::Focus { pid }) => spike::focus(pid).context("focus"),
        Command::Catalog { data_dir } => {
            let data_dir = match data_dir {
                Some(dir) => dir,
                None => tracker_api::data_dir().context("data dir")?,
            };
            let started = Instant::now();
            let (done, icons) = std::sync::mpsc::channel();
            let apps = catalog::list(&data_dir, move |written| {
                let _ = done.send(written);
            })?;
            let listed = started.elapsed();
            println!("{}", serde_json::to_string_pretty(&apps)?);
            let written = icons.recv().context("catalog thread")?;
            tracing::info!(
                "listed {} apps in {listed:?}; {written} icons written by {:?}",
                apps.len(),
                started.elapsed()
            );
            Ok(())
        }
    }
}

/// Samples on a fixed-rate deadline, like the runtime's poll thread, until stdout closes.
fn probe(watch: Watch, interval: Duration) -> anyhow::Result<()> {
    let mut sampler = WinSampler::watching(SessionWatcher::start(), watch);
    let mut stdout = std::io::stdout().lock();
    let mut deadline = Instant::now();
    loop {
        let sample = sampler.sample();
        if writeln!(stdout, "{}", serde_json::to_string(&sample)?)
            .and_then(|()| stdout.flush())
            .is_err()
        {
            // The reader went away (the harness stopped us).
            return Ok(());
        }
        deadline += interval;
        let now = Instant::now();
        if deadline < now {
            deadline = now;
        }
        std::thread::sleep(deadline - now);
    }
}
