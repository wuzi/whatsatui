use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
pub(super) struct Bridge<T> {
    tx: SyncSender<T>,
}
pub(super) fn bounded<T>(capacity: usize) -> (Bridge<T>, Receiver<T>) {
    let (tx, rx) = sync_channel(capacity);
    (Bridge { tx }, rx)
}
impl<T> Bridge<T> {
    pub fn send(&self, value: T) -> Result<(), ()> {
        let send = || self.tx.send(value).map_err(|_| ());
        if tokio::runtime::Handle::try_current()
            .is_ok_and(|h| h.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
        {
            tokio::task::block_in_place(send)
        } else {
            send()
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridge_backpressures_without_dropping() {
        let (bridge, rx) = bounded(1);
        bridge.send("first").unwrap();
        let (started, ready) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _ = started.send(());
            bridge.send("second")
        });
        ready.await.unwrap();
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(!task.is_finished(), "a full bridge must backpressure");
        assert_eq!(rx.recv().unwrap(), "first");
        assert_eq!(rx.recv().unwrap(), "second");
        assert!(task.await.unwrap().is_ok());
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn shutdown_unblocks_full_bridge() {
        let (bridge, rx) = bounded(1);
        bridge.send(1).unwrap();
        let task = tokio::spawn(async move { bridge.send(2) });
        tokio::time::sleep(std::time::Duration::from_millis(25)).await;
        assert!(!task.is_finished());
        drop(rx);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(1), task)
                .await
                .unwrap()
                .unwrap()
                .is_err()
        );
    }
}
