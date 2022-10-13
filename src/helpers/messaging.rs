//!
//! This module contains implementations and traits that enable protocols to communicate with
//! each other. In order for helpers to send messages, they need to know the destination. In some
//! cases this might be the exact address of helper host/instance (for example IP address), but
//! in many situations MPC helpers simply need to be able to send messages to the
//! corresponding helper without needing to know the exact location - this is what this module
//! enables MPC protocols to do.
//!
use crate::{
    helpers::error::Error,
    helpers::fabric::{ChannelId, CommunicationChannel, MessageEnvelope, Network},
    helpers::Identity,
    protocol::{RecordId, Step},
};
use std::collections::hash_map::Entry;
use std::collections::HashMap;

use futures::StreamExt;
use serde::{de::DeserializeOwned, Serialize};
use std::fmt::{Debug, Formatter};
use tokio::sync::{mpsc, oneshot};
use tracing::Instrument;

/// Trait for messages sent between helpers
pub trait Message: Debug + Send + Serialize + DeserializeOwned + 'static {}

impl<T> Message for T where T: Debug + Send + Serialize + DeserializeOwned + 'static {}

/// Entry point to the messaging layer managing communication channels for protocols and provides
/// the ability to send and receive messages from helper peers. Protocols request communication
/// channels to be open by calling `get_channel`, after that it is possible to send messages
/// through the channel end and request a given message type from helper peer.
///
/// Gateways are generic over `Network` meaning they can operate on top of in-memory communication
/// channels and real network.
///
/// ### Implementation details
/// Gateway, when created, runs an event loop in a dedicated tokio task that pulls the messages
/// from the networking layer and attempts to fulfil the outstanding requests to receive them.
/// If `receive` method on the channel has never been called, it puts the message to the local
/// buffer and keeps it there until such request is made by the protocol.
/// TODO: limit the size of the buffer and only pull messages when there is enough capacity
#[derive(Debug)]
pub struct Gateway<S, N> {
    helper_identity: Identity,
    network: N,
    /// Sender end of the channel to send requests to receive messages from peers.
    tx: mpsc::Sender<ReceiveRequest<S>>,
}

/// Channel end
#[derive(Debug)]
pub struct Mesh<'a, S, N> {
    network: &'a N,
    step: S,
    helper_identity: Identity,
    gateway_tx: mpsc::Sender<ReceiveRequest<S>>,
}

/// Local buffer for messages that are either awaiting requests to receive them or requests
/// that are pending message reception.
/// TODO: Right now it is backed by a hashmap but `SipHash` (default hasher) performance is not great
/// when protection against collisions is not required, so either use a vector indexed by
/// an offset + record or [xxHash](https://github.com/Cyan4973/xxHash)
#[derive(Debug, Default)]
struct MessageBuffer {
    buf: HashMap<RecordId, BufItem>,
}

#[derive(Debug)]
enum BufItem {
    /// There is an outstanding request to receive the message but this helper hasn't seen it yet
    Requested(oneshot::Sender<Box<[u8]>>),
    /// Message has been received but nobody requested it yet
    Received(Box<[u8]>),
}

struct ReceiveRequest<S> {
    channel_id: ChannelId<S>,
    record_id: RecordId,
    sender: oneshot::Sender<Box<[u8]>>,
}

impl<S: Step, F: Network<S>> Mesh<'_, S, F> {
    /// Send a given message to the destination. This method will not return until the message
    /// is delivered to the `Network`.
    ///
    /// # Errors
    /// Returns an error if it fails to send the message or if there is a serialization error.
    pub async fn send<T: Message>(
        &mut self,
        dest: Identity,
        record_id: RecordId,
        msg: T,
    ) -> Result<(), Error> {
        let channel = self
            .network
            .get_connection(ChannelId::new(dest, self.step))
            .await;
        let bytes = serde_json::to_vec(&msg)
            .map_err(|e| Error::serialization_error(record_id, self.step, e))?
            .into_boxed_slice();

        let envelope = MessageEnvelope {
            record_id,
            payload: bytes,
        };

        channel.send(envelope).await
    }

    /// Receive a message that is associated with the given record id.
    ///
    /// # Errors
    /// Returns an error if it fails to receive the message or if a deserialization error occurred
    pub async fn receive<T: Message>(
        &mut self,
        source: Identity,
        record_id: RecordId,
    ) -> Result<T, Error> {
        let (tx, rx) = oneshot::channel();

        self.gateway_tx
            .send(ReceiveRequest {
                channel_id: ChannelId::new(source, self.step),
                record_id,
                sender: tx,
            })
            .await
            .map_err(|e| Error::receive_error(source, e.to_string()))?;

        let payload = rx.await.map_err(|e| Error::receive_error(source, e))?;
        let obj: T = serde_json::from_slice(&payload)
            .map_err(|e| Error::serialization_error(record_id, self.step, e))?;

        Ok(obj)
    }

    /// Returns the unique identity of this helper.
    pub fn identity(&self) -> Identity {
        self.helper_identity
    }
}

impl<S: Step, N: Network<S>> Gateway<S, N> {
    pub fn new(identity: Identity, network: N) -> Self {
        let (tx, mut receive_rx) = mpsc::channel::<ReceiveRequest<S>>(1);
        let mut message_stream = network.message_stream();

        tokio::spawn(async move {
            let mut buf = HashMap::<ChannelId<S>, MessageBuffer>::new();

            loop {
                // Make a random choice what to process next:
                // * Receive a message from another helper
                // * Handle the request to receive a message from another helper
                tokio::select! {
                    Some(receive_request) = receive_rx.recv() => {
                        tracing::trace!("new {:?}", receive_request);
                        buf.entry(receive_request.channel_id)
                           .or_default()
                           .receive_request(receive_request.record_id, receive_request.sender);
                    }
                    Some((channel_id, messages)) = message_stream.next() => {
                        tracing::trace!("received {} message(s) from {:?}", messages.len(), channel_id);
                        buf.entry(channel_id)
                           .or_default()
                           .receive_messages(messages);
                    }
                    else => {
                        tracing::debug!("All channels are closed and event loop is terminated");
                        break;
                    }
                }
            }
        }.instrument(tracing::info_span!("gateway_event_loop", identity=?identity)));

        Self {
            helper_identity: identity,
            network,
            tx,
        }
    }

    /// Create or return an existing channel for a given step. Protocols can send messages to
    /// any helper through this channel (see `Mesh` interface for details).
    ///
    /// This method makes no guarantee that the communication channel will actually be established
    /// between this helper and every other one. The actual connection may be created only when
    /// `Mesh::send` or `Mesh::receive` methods are called.
    pub fn get_channel(&self, step: S) -> Mesh<'_, S, N> {
        Mesh {
            network: &self.network,
            helper_identity: self.helper_identity,
            step,
            gateway_tx: self.tx.clone(),
        }
    }
}

impl MessageBuffer {
    /// Process request to receive a message with the given `RecordId`.
    fn receive_request(&mut self, record_id: RecordId, s: oneshot::Sender<Box<[u8]>>) {
        match self.buf.entry(record_id) {
            Entry::Occupied(entry) => match entry.remove() {
                BufItem::Requested(_) => {
                    panic!("More than one request to receive a message for {record_id:?}");
                }
                BufItem::Received(payload) => {
                    s.send(payload).unwrap_or_else(|_| {
                        tracing::warn!("No listener for message {record_id:?}");
                    });
                }
            },
            Entry::Vacant(entry) => {
                entry.insert(BufItem::Requested(s));
            }
        }
    }

    /// Process message that has been received
    fn receive_message(&mut self, msg: MessageEnvelope) {
        match self.buf.entry(msg.record_id) {
            Entry::Occupied(entry) => match entry.remove() {
                BufItem::Requested(s) => {
                    s.send(msg.payload).unwrap_or_else(|_| {
                        tracing::warn!("No listener for message {:?}", msg.record_id);
                    });
                }
                BufItem::Received(_) => {
                    panic!("Duplicate message for the same record {:?}", msg.record_id);
                }
            },
            Entry::Vacant(entry) => {
                entry.insert(BufItem::Received(msg.payload));
            }
        }
    }

    fn receive_messages(&mut self, msgs: Vec<MessageEnvelope>) {
        for msg in msgs {
            self.receive_message(msg);
        }
    }
}

impl<S: Step> Debug for ReceiveRequest<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "ReceiveRequest({:?}, {:?})",
            self.channel_id, self.record_id
        )
    }
}