use std::collections::HashMap;
use std::ops::DerefMut;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::document::SubscribableDocument;
use crate::ephemerial_operation::EphemerialOperation;
use crate::node_inner::MessageType;
use crate::node_inner::NodeInner;
use chrono::Utc;
use p2panda_core::cbor::{DecodeError, decode_cbor, encode_cbor};
use p2panda_core::{PrivateKey, PublicKey};
use p2panda_net::ToNetwork;
use tokio::{
    sync::mpsc,
    sync::{Mutex, RwLock},
};
use tracing::error;

const OFFLINE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum AuthorMessage {
    Hello,
    Ping,
    Bye,
}

impl std::fmt::Display for AuthorMessage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AuthorMessage::Hello => write!(f, "Hello message"),
            AuthorMessage::Ping => write!(f, "Ping message"),
            AuthorMessage::Bye => write!(f, "Bye message"),
        }
    }
}

impl TryFrom<&[u8]> for AuthorMessage {
    type Error = DecodeError;

    fn try_from(value: &[u8]) -> Result<Self, Self::Error> {
        let (res, _): (AuthorMessage, SystemTime) = decode_cbor(value)?;

        Ok(res)
    }
}

pub struct AuthorTracker<T> {
    last_ping: Mutex<HashMap<PublicKey, Instant>>,
    document: Arc<T>,
    node: Arc<NodeInner>,
    tx: RwLock<Option<mpsc::Sender<ToNetwork>>>,
}

impl<T: SubscribableDocument> AuthorTracker<T> {
    pub fn new(node: Arc<NodeInner>, document: Arc<T>) -> Arc<Self> {
        Arc::new(Self {
            last_ping: Mutex::new(HashMap::new()),
            document,
            node,
            tx: RwLock::new(None),
        })
    }

    pub async fn set_document_tx(&self, tx: Option<mpsc::Sender<ToNetwork>>) {
        let mut tx_guard = self.tx.write().await;
        // Send good bye message to the network
        if let Some(tx) = tx_guard.as_ref() {
            send_message(&self.node.private_key, tx, AuthorMessage::Bye).await;
        }

        // Set all authors that the tracker has seen to offline, authors the tracker hasn't seen are already offline
        let old_authors =
            std::mem::replace(self.last_ping.lock().await.deref_mut(), HashMap::new());
        for author in old_authors.into_keys() {
            self.document.author_left(author);
            self.set_last_seen(author).await;
        }

        *tx_guard = tx;
    }

    pub async fn received(&self, message: AuthorMessage, author: PublicKey) {
        match message {
            AuthorMessage::Hello => {
                self.join(author).await;
            }
            AuthorMessage::Ping => {
                self.ping(author).await;
            }
            AuthorMessage::Bye => {
                self.left(author).await;
            }
        }
    }

    async fn send(&self, message: AuthorMessage) {
        if let Some(tx) = self.tx.read().await.as_ref() {
            send_message(&self.node.private_key, tx, message).await;
        }
    }

    async fn join(&self, author: PublicKey) {
        self.last_ping.lock().await.insert(author, Instant::now());
        self.document.author_joined(author);

        // Send a ping to the network to ensure that the new author knows we exist
        // Normally we send a ping every `OFFLINE_TIMEOUT / 2`
        self.send(AuthorMessage::Ping).await;
    }

    async fn ping(&self, author: PublicKey) {
        let old = self.last_ping.lock().await.insert(author, Instant::now());

        // If this is a new author emit author join
        if old.is_none() {
            self.document.author_joined(author);
        }
    }

    async fn left(&self, author: PublicKey) {
        self.last_ping.lock().await.remove(&author);
        self.document.author_left(author);
        self.set_last_seen(author).await;
    }

    pub async fn spawn(&self) {
        // Send a hello to the network so other authors know we joined the document
        self.send(AuthorMessage::Hello).await;

        let mut interval = tokio::time::interval(OFFLINE_TIMEOUT / 2);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip over the first tick which completes immediately
        interval.tick().await;

        loop {
            interval.tick().await;

            // Send a ping to the network so that we won't be marked as offline
            self.send(AuthorMessage::Ping).await;
            let mut expired = Vec::new();
            self.last_ping.lock().await.retain(|author, instant| {
                if instant.elapsed() > OFFLINE_TIMEOUT {
                    expired.push(*author);
                    false
                } else {
                    true
                }
            });

            for author in expired {
                self.document.author_left(author);
                self.set_last_seen(author).await;
            }
        }
    }

    async fn set_last_seen(&self, author: PublicKey) {
        if let Err(error) = self
            .node
            .document_store
            .set_last_seen_for_author(author, Some(Utc::now()))
            .await
        {
            error!("Failed to set last seen for author {author}: {error}");
        }
    }
}

async fn send_message(
    private_key: &PrivateKey,
    tx: &mpsc::Sender<ToNetwork>,
    message: AuthorMessage,
) {
    // FIXME: We need to add the current time to the message,
    // because iroh doesn't broadcast twice the same message message.
    let author_message = match encode_cbor(&(&message, SystemTime::now())) {
        Ok(result) => result,
        Err(error) => {
            error!("Failed to encode {message} as CBOR: {error}");
            return;
        }
    };
    let operation = EphemerialOperation::new(author_message, private_key);
    let bytes = match encode_cbor(&MessageType::AuthorEphemeral(operation)) {
        Ok(result) => result,
        Err(error) => {
            error!("Failed to encode {message} as CBOR: {error}");
            return;
        }
    };
    if let Err(error) = tx.send(ToNetwork::Message { bytes }).await {
        error!("Failed to sent {message} to the network: {error}");
    }
}
