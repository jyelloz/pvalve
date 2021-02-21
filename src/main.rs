use std::thread;
use std::io::{
    self,
    copy,
    Result,
    Stdin,
    Stdout,
};
use std::sync::mpsc::Sender;
use std::num::NonZeroU32;
use nonzero_ext::nonzero;

use pvalve::{
    syncio::RateLimitedWriter,
    tui::{
        user_interface,
        cleanup,
    },
};
use crossterm::tty::IsTty;


fn main() -> anyhow::Result<()> {
    let mut stdin = io::stdin();
    let stdout = io::stdout();
    let (tx, rx) = std::sync::mpsc::channel();
    let ui = if !stdin.is_tty() && !stdout.is_tty() {
        Some(thread::spawn(move || user_interface(tx)))
    } else {
        None
    };
    let mut stdout = RateLimitedWriter::writer_with_rate_and_updates(
        stdout,
        nonzero!(1u32),
        rx
    );
    let copy_result = copy(&mut stdin, &mut stdout);
    if ui.is_some() {
        cleanup()?;
    }
    copy_result?;
    Ok(())
}
