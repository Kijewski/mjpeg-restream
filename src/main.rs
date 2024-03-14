mod listener;
mod multipart_stream_fixed;
mod sender;
mod update_stream;

use std::process::abort;

use bytes::Bytes;
use once_cell::sync::OnceCell;
use tokio::select;
use tokio::sync::oneshot;

use self::update_stream::UpdateStream;

#[tokio::main(flavor = "current_thread")]
async fn main() -> anyhow::Result<()> {
    let (tx, rx) = oneshot::channel();
    let mut tx = Some(tx);
    ctrlc::try_set_handler(move || {
        eprintln!("Trapped shutdown signal.");
        let Some(tx) = tx.take() else {
            eprint!("Caught shutdown signal twice. Aborting.");
            abort();
        };
        let _ = tx.send(());
    })?;

    select! {
        biased;
        _ = rx => Ok(()),
        _ = listener::listener() => panic!("Listener finished"),
        _ = sender::sender() => panic!("Sender finished"),
    }
}

fn image_holder() -> &'static UpdateStream<Bytes> {
    static HOLDER: OnceCell<UpdateStream<Bytes>> = OnceCell::new();
    HOLDER.get_or_init(UpdateStream::default)
}
