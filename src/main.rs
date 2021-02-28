use nonzero_ext::nonzero;
use std::{
    io::{self, copy},
    sync::mpsc::channel,
    thread,
};

use crossterm::tty::IsTty;
use pvalve::{
    ipc::ProgressMessage,
    memslot::Memslot,
    syncio::RateLimitedWriter,
    tui::{Cleanup, UserInterface},
    cli::Opts,
};

fn main() -> anyhow::Result<()> {

    let invo = Opts::parse_process_args();

    let rate = invo.speed.map(|s| s.0).unwrap_or(nonzero!(1u32));

    let mut stdin = io::stdin();
    let stdout = io::stdout();
    let (mut state_tx, state_rx) =
        Memslot::new(ProgressMessage::Initial).split();
    let (control_tx, control_rx) = channel();
    let ui = if !stdin.is_tty() && !stdout.is_tty() {
        let ui = UserInterface::new(control_tx, state_rx)?;
        Some(thread::spawn(move || ui.run(rate)))
    } else {
        None
    };
    let mut stdout = RateLimitedWriter::writer_with_rate_and_updates(
        stdout,
        rate,
        control_rx,
        state_tx.clone(),
    );
    let copy_result = copy(&mut stdin, &mut stdout);
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
