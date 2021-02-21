use nonzero_ext::nonzero;
use std::{
    io::{self, copy},
    sync::mpsc::channel,
    thread,
};

use crossterm::tty::IsTty;
use pvalve::{
    syncio::RateLimitedWriter,
    tui::{cleanup, user_interface},
};

fn main() -> anyhow::Result<()> {
    let mut stdin = io::stdin();
    let stdout = io::stdout();
    let (tx, rx) = channel();
    let ui = if !stdin.is_tty() && !stdout.is_tty() {
        Some(thread::spawn(move || user_interface(tx)))
    } else {
        None
    };
    let mut stdout = RateLimitedWriter::writer_with_rate_and_updates(
        stdout,
        nonzero!(1u32),
        rx,
    );
    let copy_result = copy(&mut stdin, &mut stdout);
    if ui.is_some() {
        cleanup()?;
    }
    copy_result?;
    Ok(())
}
