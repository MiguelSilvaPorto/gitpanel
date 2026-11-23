use std::future::Future;
use std::sync::mpsc::{Receiver, channel};

pub struct Jobs {
    runtime: tokio::runtime::Runtime,
}

impl Jobs {
    pub fn new() -> std::io::Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()?;
        Ok(Self { runtime })
    }

    pub fn spawn<F, T>(&self, future: F) -> Receiver<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let (sender, receiver) = channel();
        self.runtime.spawn(async move {
            let _ = sender.send(future.await);
        });
        receiver
    }
}
