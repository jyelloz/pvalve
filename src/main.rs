use std::path::Path;

use tokio::sync::watch::{
    channel,
    Sender,
    Receiver,
};
use tokio::io::{
    self,
    BufReader,
    AsyncBufReadExt,
    AsyncRead,
    AsyncReadExt,
    AsyncWrite,
    AsyncWriteExt,
};
use tokio::fs::{
    File,
    OpenOptions,
};
use tokio::time::sleep;
use leaky_bucket::LeakyBucket;
use std::time::Duration;

async fn watch_control_file(path: &str, tx: Sender<usize>) -> io::Result<()> {
    let path = Path::new(&path);
    {
        OpenOptions::new()
            .write(true)
            .create(true)
            .open(path)
            .await?;
    }
    loop {
        let file = File::open(path).await?;
        let file = BufReader::new(file);
        let line = file.lines().next_line().await?;
        let line = line.unwrap_or_default();
        let rate: Option<usize> = line.parse().ok();
        match rate {
            Some(rate) => {
                tx.send(rate).expect("failed to update rate");
            },
            _ => {
                eprintln!("could not parse control file");
            },
        }
        sleep(Duration::from_secs(1)).await;
    }
}


fn get_bucket(rate: usize) -> LeakyBucket {
    LeakyBucket::builder()
        .max(rate)
        .tokens(0)
        .refill_interval(Duration::from_secs(1))
        .refill_amount(rate)
        .build()
        .expect("failed to build leaky bucket")
}

async fn transfer<I: AsyncRead + Unpin, O: AsyncWrite + Unpin>(
    mut rx: I,
    mut tx: O,
    buffer_length: usize,
    control: Receiver<usize>,
) -> io::Result<()> {

    let mut bucket = get_bucket(*control.borrow());
    let mut buf = vec![0u8; buffer_length];
    let buf = &mut buf.as_mut_slice()[0..buffer_length];
    loop {

        let new_rate = *control.borrow();
        if new_rate != bucket.max() {
            bucket = get_bucket(new_rate);
        }

        let bytes_read = rx.read(buf).await?;
        if bytes_read == 0 {
            break;
        }

        tx.write_all(&buf[0..bytes_read]).await?;

        tx.flush().await?;

        bucket.acquire(bytes_read).await.ok();

    }
    Ok(())
}

#[tokio::main]
async fn main() -> io::Result<()> {
    let buffer_length = 1 << 20;
    let (tx, rx) = channel(1024usize);
    let watch = tokio::spawn(
        watch_control_file("control", tx)
    );
    tokio::spawn(
        transfer(
            io::stdin(),
            io::stdout(),
            buffer_length,
            rx,
        )
    ).await??;
    watch.abort();
    Ok(())
}
