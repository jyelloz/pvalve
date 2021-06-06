use nonzero_ext::nonzero;
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
    ipc::ProgressMessage,
    memslot::Memslot,
    syncio::{RateLimitedWriter, WriteExt as _},
    tui::{Cleanup, UserInterface},
};

fn main() -> anyhow::Result<()> {
    let invo = Opts::parse_process_args();

    let rate = invo.speed.map(|s| s.0).unwrap_or(nonzero!(1u32));

    let config = Config {
        limit: Some(rate),
        ..Default::default()
    };

    let (watch_tx, config_watch) = ConfigMonitor::new(config.clone());

    let stdin = io::stdin();
    let stdout = io::stdout();
    let (mut state_tx, state_rx) =
        Memslot::new(ProgressMessage::Initial).split();

    let mut paused = Latch::new();
    let mut aborted = Latch::new();

    let interactive_mode = !stdin.is_tty() && !stdout.is_tty();
    let mut stdout = RateLimitedWriter::writer_with_config(
        stdout,
        config_watch,
    )
        .pauseable(paused.watch())
        .cancellable(aborted.watch())
        .progress();
    let ui = if interactive_mode {
        let ui = UserInterface::new(
            paused,
            aborted,
            state_rx,
            config.clone(),
            stdout.bytes_transferred(),
            watch_tx,
        )?;
        Some(thread::spawn(|| ui.run(Instant::now())))
    } else {
        None
    };
    let copy_result = copy(&mut stdin.lock(), &mut stdout);
    state_tx.set(ProgressMessage::Interrupted);
    if let Some(ui) = ui {
        match ui.join() {
            Err(_) | Ok(Err(_)) => {
                Cleanup();
            }
            _ => {}
        }
    }
    copy_result?;
    Ok(())
}
