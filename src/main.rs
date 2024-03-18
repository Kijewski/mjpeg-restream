mod listener;
mod multipart_stream_fixed;
mod sender;
mod update_stream;

use std::process::abort;

use bytes::Bytes;
use clap::Parser;
use once_cell::sync::OnceCell;
use tokio::select;
use tokio::sync::oneshot;

use self::listener::listener;
use self::sender::sender;
use self::update_stream::UpdateStream;

fn main() -> Result<(), Error> {
    let args = Args::parse();

    let (tx, rx) = oneshot::channel();
    let mut tx = Some(tx);
    ctrlc::try_set_handler(move || trapped_ctrl_c(&mut tx)).map_err(Error::CtrlC)?;

    let listener = listener(args.listener);
    let sender = sender(args.sender);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::Rt)?
        .block_on(async move {
            select! {
                biased;
                _ = rx => (),
                result = listener => result.map_err(Error::Listener)?,
                result = sender => result.map_err(Error::Sender)?,
            }
            Ok(())
        })
}

fn trapped_ctrl_c(tx: &mut Option<oneshot::Sender<()>>) {
    eprintln!("Trapped shutdown signal.");
    let Some(tx) = tx.take() else {
        eprint!("Caught shutdown signal twice. Aborting.");
        abort();
    };
    let _ = tx.send(());
}

fn image_holder() -> &'static UpdateStream<Bytes> {
    static HOLDER: OnceCell<UpdateStream<Bytes>> = OnceCell::new();
    HOLDER.get_or_init(Default::default)
}

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
#[group(id = "crate")]
struct Args {
    #[command(flatten)]
    listener: self::listener::Args,
    #[command(flatten)]
    sender: self::sender::Args,
}

#[derive(thiserror::Error, pretty_error_debug::Debug)]
enum Error {
    #[error("Could not start Tokio runtime")]
    Rt(#[source] std::io::Error),
    #[error("Could not set Ctrl+C handler")]
    CtrlC(#[source] ctrlc::Error),
    #[error("The server part failed")]
    Sender(#[source] self::sender::Error),
    #[error("The client part failed")]
    Listener(#[source] self::listener::Error),
}
