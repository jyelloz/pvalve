use nonzero_ext::nonzero;
use std::{
    io::{self, copy},
    sync::mpsc::channel,
    thread,
};

use crossterm::tty::IsTty;
use pvalve::{
    ipc::ProgressMessage,
    syncio::RateLimitedWriter,
    tui::{Cleanup, UserInterface},
};

fn main() -> anyhow::Result<()> {
    let mut stdin = io::stdin();
    let stdout = io::stdout();
    let (state_tx, state_rx) = channel();
    let (control_tx, control_rx) = channel();
    let ui = if !stdin.is_tty() && !stdout.is_tty() {
        let ui = UserInterface::new(control_tx, state_rx)?;
        Some(thread::spawn(move || ui.run()))
    } else {
        None
    };
    let mut stdout = RateLimitedWriter::writer_with_rate_and_updates(
        stdout,
        nonzero!(1u32),
        control_rx,
        state_tx.clone(),
    );
    let copy_result = copy(&mut stdin, &mut stdout);
    state_tx.send(ProgressMessage::Interrupted)?;
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
