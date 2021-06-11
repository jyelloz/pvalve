use std::io::{Result, copy, stdin, stdout};
use std::thread::{
    spawn,
    JoinHandle,
};

use watch::{WatchSender, WatchReceiver};

pub struct PipeValveTask {
    copy: JoinHandle<Result<u64>>,
}

impl PipeValveTask {

    fn from(handle: JoinHandle<Result<u64>>) -> Self {
        Self { copy: handle }
    }

    fn join(self) -> Result<u64> {
        let result = self.copy.join();
        match result {
            Ok(Ok(i)) => Ok(i),
            _ => panic!("failed to await copy"),
        }
    }

    pub fn cat() -> Self {
        Self {
            copy: spawn(|| {
                let stdin = stdin();
                let stdout = stdout();
                let result = copy(&mut stdin.lock(), &mut stdout.lock());
                result
            }),
        }
    }

}

#[cfg(test)]
mod test {
    use super::*;
    use std::io::Read;

    struct Dummy(usize);

    impl Read for Dummy {
        fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
            if self.0 < 1 {
                return Ok(0);
            }
            self.0 = usize::min(self.0, self.0 - 1);
            let text = "text!\n".as_bytes();
            let bytes_to_read = usize::min(text.len(), buf.len());
            buf[..bytes_to_read].copy_from_slice(&text[..bytes_to_read]);
            Ok(bytes_to_read)
        }
    }

    #[test]
    fn test() {
        use std::thread::sleep;
        use std::time::Duration;
        let handle: JoinHandle<Result<u64>> = spawn(|| {
            sleep(Duration::from_secs(10));
            Ok(0)
        });
        let task = PipeValveTask::from(handle);
        let r = task.join().expect("failed to await task");
        dbg!(&r);
    }
    #[test]
    fn test_stdout() {
        let handle: JoinHandle<Result<u64>> = spawn(|| {
            let mut d = Dummy(10);
            copy(&mut d, &mut std::io::stdout())
        });
        let task = PipeValveTask::from(handle);
        let r = task.join().expect("failed to await task");
        dbg!(&r);
    }
}
