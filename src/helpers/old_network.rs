use crate::helpers::error::Error;
/// The only usage of this module is in `net` module that is awaiting to be migrated to `Transport`
/// interface.
use crate::helpers::network::MessageChunks;
use async_trait::async_trait;
use futures::{ready, Stream};
use pin_project::pin_project;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::mpsc;
use tokio_util::sync::{PollSendError, PollSender};

/// Network interface for components that require communication.
#[async_trait]
pub trait Network: Sync {
    /// Type of the channel that is used to send messages to other helpers
    type Sink: futures::Sink<MessageChunks, Error = Error> + Send + Unpin + 'static;
    type MessageStream: Stream<Item = MessageChunks> + Send + Unpin + 'static;

    /// Returns a sink that accepts data to be sent to other helper parties.
    fn sink(&self) -> Self::Sink;

    /// Returns a stream to receive messages that have arrived from other helpers. Note that
    /// some implementations may panic if this method is called more than once.
    fn recv_stream(&self) -> Self::MessageStream;
}

/// Wrapper around a [`PollSender`] to modify the error message to match what the [`NetworkSink`]
/// requires. The only error that [`PollSender`] will generate is "channel closed", and thus is the
/// only error message forwarded from this [`NetworkSink`].
#[pin_project]
pub struct NetworkSink<T> {
    #[pin]
    inner: PollSender<T>,
}

impl<T: Send + 'static> NetworkSink<T> {
    #[must_use]
    pub fn new(sender: mpsc::Sender<T>) -> Self {
        Self {
            inner: PollSender::new(sender),
        }
    }
}

impl<T: Send + 'static> futures::Sink<T> for NetworkSink<T>
where
    Error: From<PollSendError<T>>,
{
    type Error = Error;

    fn poll_ready(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.project().inner.poll_ready(cx)?);
        Poll::Ready(Ok(()))
    }

    fn start_send(self: Pin<&mut Self>, item: T) -> Result<(), Self::Error> {
        self.project().inner.start_send(item)?;
        Ok(())
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.project().inner.poll_flush(cx)?);
        Poll::Ready(Ok(()))
    }

    fn poll_close(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        ready!(self.project().inner.poll_close(cx))?;
        Poll::Ready(Ok(()))
    }
}