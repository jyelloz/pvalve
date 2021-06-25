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

    let config = Config { limit, unit };

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
        .progress();
    let ui = if interactive_mode {
        let ui = UserInterface::new(
            paused,
            aborted,
            shutdown.watch(),
            config,
            stdout.transfer_progress(),
            config_tx,
        )?;
        Some(thread::spawn(|| ui.run(Instant::now())))
    } else {
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
