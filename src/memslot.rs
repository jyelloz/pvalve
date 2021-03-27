use std::sync::{Arc, RwLock};

pub struct Memslot<T: Clone + Send + Sync> {
    inner: Arc<RwLock<T>>,
}

#[derive(Clone)]
pub struct ReadHalf<T: Clone + Send + Sync>(Arc<RwLock<T>>);
#[derive(Clone)]
pub struct WriteHalf<T: Clone + Send + Sync>(Arc<RwLock<T>>);

impl<T> Memslot<T>
where
    T: Clone + Send + Sync,
{
    pub fn new(value: T) -> Self {
        Self {
            inner: Arc::new(RwLock::new(value)),
        }
    }
    pub fn split(self) -> (WriteHalf<T>, ReadHalf<T>) {
        let read = ReadHalf(self.inner.clone());
        let write = WriteHalf(self.inner.clone());
        (write, read)
    }
}

impl<T> ReadHalf<T>
where
    T: Clone + Send + Sync,
{
    pub fn get(&self) -> T {
        self.0.read().unwrap().clone()
    }
}
impl<T> WriteHalf<T>
where
    T: Clone + Send + Sync,
{
    pub fn set(&mut self, new: T) {
        *self.0.write().unwrap() = new;
    }
}
