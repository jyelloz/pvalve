use pvalve::syncio;
use std::io;
use nonzero_ext::nonzero;

fn main() -> io::Result<()> {
    let mut stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout = syncio::RateLimitedWriter::writer_with_rate(
        stdout,
        nonzero!(10u32),
    );
    io::copy(&mut stdin, &mut stdout)?;
    Ok(())
}
