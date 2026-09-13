//! One bounded database queue; SQLite never blocks an async executor thread.
use super::Store;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
type Task = Box<dyn FnOnce(&mut Store) + Send>;
#[derive(Clone)]
pub struct Worker(mpsc::Sender<Task>);
impl Worker {
    pub async fn open(path: PathBuf) -> anyhow::Result<Self> {
        let (tx, mut rx) = mpsc::channel::<Task>(64);
        let (ready, started) = oneshot::channel();
        std::thread::Builder::new()
            .name("frpsh-spaces-db".into())
            .spawn(move || {
                let mut db = match Store::open(&path) {
                    Ok(db) => db,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                if ready.send(Ok(())).is_err() {
                    return;
                }
                while let Some(task) = rx.blocking_recv() {
                    task(&mut db);
                }
            })?;
        started.await??;
        Ok(Self(tx))
    }
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Store) -> anyhow::Result<T> + Send + 'static,
    ) -> anyhow::Result<T> {
        let (tx, rx) = oneshot::channel();
        self.0
            .try_send(Box::new(move |db| {
                let _ = tx.send(f(db));
            }))
            .map_err(|_| anyhow::anyhow!("space database busy or unavailable"))?;
        rx.await?
    }
}
