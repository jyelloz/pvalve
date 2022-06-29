use std::{
    io::{self, copy},
    time::Instant,
    thread,
};

use crossterm::tty::IsTty;
use pvalve::{
    config::{
        Config,
        ConfigMonitor,
        Latch,
    },
    cli::Opts,
    syncio::WriteExt as _,
    tui::{Cleanup, UserInterface},
};

fn main() -> anyhow::Result<()> {
    let invo = Opts::parse_process_args();

    let limit = invo.speed.map(|s| s.0).into();
    let unit = invo.unit;
    let expected_size = invo.expected_size;

    let config = Config { limit, unit, expected_size };

    let (config_tx, config_rx) = ConfigMonitor::new(config);

    let stdin = io::stdin();
    let stdout = io::stdout();

    let mut shutdown = Latch::new();
    let mut paused = Latch::new();
    let mut aborted = Latch::new();

    let interactive_mode = !stdin.is_tty() && !stdout.is_tty();
    let mut stdout = stdout.limited(config_rx)
        .pauseable(paused.watch())
        .cancellable(aborted.watch())
        .instantaneous(std::time::Duration::from_secs(1));
    let instantaneous_progress = stdout.transfer_progress();
    let mut stdout = stdout.progress();
    let absolute_progress = stdout.transfer_progress();
    let ui = if interactive_mode {
        let ui = UserInterface::new(
            paused,
            aborted,
            shutdown.watch(),
            config,
            absolute_progress,
            instantaneous_progress,
            config_tx,
        )?;
        Some(thread::spawn(|| ui.run(Instant::now())))
    } else {
        eprintln!(
            "!!! INTERACTIVE MODE DISABLED: \
            either stdin or stdout is not a tty !!!"
        );
        None
    };
    let copy_result = copy(&mut stdin.lock(), &mut stdout);
    shutdown.on();
    if let Some(ui) = ui {
        match ui.join() {
            Err(_) | Ok(Err(_)) => { Cleanup(); }
            _ => {}
        }
    }
    copy_result?;
    Ok(())
}
