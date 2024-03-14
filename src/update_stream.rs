use std::num::NonZeroU64;

use async_condvar_fair::{BatonExt, Condvar};
use futures_util::stream::unfold;
use futures_util::Stream;
use tokio::sync::RwLock;

#[derive(Default)]
pub struct UpdateStream<T: Send + Sync + Clone> {
    holder: RwLock<Option<(NonZeroU64, T)>>,
    cv: Condvar,
}

impl<T: Send + Sync + Clone> UpdateStream<T> {
    pub fn stream_updates<'a>(&'a self) -> impl 'a + Stream<Item = T> {
        unfold((self, 0), bytes_event_listen)
    }

    pub async fn update(&self, new_data: T) {
        let mut guard = self.holder.write().await;
        let idx = guard.as_ref().map_or(0, |(idx, _)| idx.get());
        *guard = Some((NonZeroU64::new(idx + 1).unwrap(), new_data));
        drop(guard);
        self.cv.notify_all();
    }
}

async fn bytes_event_listen<T: Send + Sync + Clone>(
    (inner, idx): (&UpdateStream<T>, u64),
) -> Option<(T, (&UpdateStream<T>, u64))> {
    let mut guard = inner.holder.read().await;
    let mut baton = None;
    loop {
        if let &Some((cur_idx, ref bytes)) = &*guard {
            if cur_idx.get() > idx {
                let datum = bytes.clone();
                drop(baton);
                drop(guard);
                return Some((datum, (inner, cur_idx.get())));
            }
        }

        baton.dispose();
        (guard, baton) = inner.cv.wait_baton((guard, &inner.holder)).await;
    }
}
