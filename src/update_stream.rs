use std::num::NonZeroU64;

use async_condvar_fair::Condvar;
use futures_util::Stream;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct UpdateStream<T: Send + Sync + Clone> {
    holder: RwLock<Option<(NonZeroU64, T)>>,
    cv: Condvar,
}

impl<T: Send + Sync + Clone> UpdateStream<T> {
    pub fn stream_updates(&self) -> impl '_ + Stream<Item = T> {
        async_stream::stream! {
            let mut idx = 0;
            loop {
                let guard = self.holder.read().await;
                if let Some((cur_idx, ref bytes)) = &*guard {
                    let cur_idx = cur_idx.get();
                    if cur_idx > idx {
                        idx = cur_idx;
                        yield bytes.clone();
                    }
                }
                let _ = self.cv.wait_no_relock((guard, &self.holder)).await;
            }
        }
    }

    pub async fn update(&self, new_data: T) {
        let mut guard = self.holder.write().await;
        let idx = guard.as_ref().map_or(0, |(idx, _)| idx.get());
        *guard = Some((NonZeroU64::new(idx + 1).unwrap(), new_data));
        drop(guard);
        self.cv.notify_all();
    }
}
