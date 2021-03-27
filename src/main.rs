use nonzero_ext::nonzero;
use std::{
    io::{self, copy},
    sync::mpsc::channel,
    time::Instant,
    thread,
};

use crossterm::tty::IsTty;
use pvalve::{
    config::Config,
    cli::Opts,
    ipc::ProgressMessage,
    memslot::Memslot,
    syncio::{RateLimitedWriter, WithProgress as _},
    tui::{Cleanup, UserInterface},
};

fn main() -> anyhow::Result<()> {
    let invo = Opts::parse_process_args();

    let rate = invo.speed.map(|s| s.0).unwrap_or(nonzero!(1u32));

    let config = Config {
        limit: Some(rate),
        ..Default::default()
    };

    config.make_current();

    let stdin = io::stdin();
    let stdout = io::stdout();
    let (mut state_tx, state_rx) =
        Memslot::new(ProgressMessage::Initial).split();

    let (control_tx, control_rx) = channel();
    let interactive_mode = !stdin.is_tty() && !stdout.is_tty();
    let mut stdout = RateLimitedWriter::writer_with_rate_and_updates(
        stdout,
        rate,
        control_rx,
    ).progress();
    let ui = if interactive_mode {
        let ui = UserInterface::new(
            control_tx,
            state_rx,
            stdout.bytes_transferred(),
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
