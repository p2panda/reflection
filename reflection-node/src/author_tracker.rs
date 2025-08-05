use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use crate::document::{DocumentError, SubscribableDocument};
use crate::ephemerial_operation::EphemerialOperation;
use crate::node_inner::MessageType;
use crate::node_inner::NodeInner;
use chrono::Utc;
use p2panda_core::PublicKey;
use p2panda_core::{
    PrivateKey,
    cbor::{DecodeError, decode_cbor, encode_cbor},
};
use p2panda_net::ToNetwork;
use tokio::{sync::Mutex, sync::mpsc};
use tracing::error;

const OFFLINE_TIMEOUT: Duration = Duration::from_secs(60);

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub enum AuthorMessage {
    Hello,
    Ping,
    Bye,
}

impl AuthorMessage {
    pub async fn send(
        &self,
        tx: &mpsc::Sender<ToNetwork>,
        private_key: &PrivateKey,
    ) -> Result<(), DocumentError> {
        // FIXME: We need to add the current time to the message,
        // because iroh doesn't broadcast twice the same message message.
        let author_message = encode_cbor(&(self, SystemTime::now()))?;
        let operation = EphemerialOperation::new(author_message, private_key);
        let bytes = encode_cbor(&MessageType::AuthorEphemeral(operation))?;
        tx.send(ToNetwork::Message { bytes }).await?;

        Ok(())
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
    tx: mpsc::Sender<ToNetwork>,
}

impl<T: SubscribableDocument> AuthorTracker<T> {
    pub fn new(node: Arc<NodeInner>, document: Arc<T>, tx: mpsc::Sender<ToNetwork>) -> Arc<Self> {
        Arc::new(Self {
            last_ping: Mutex::new(HashMap::new()),
            document,
            node,
            tx,
        })
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

    async fn join(&self, author: PublicKey) {
        self.last_ping.lock().await.insert(author, Instant::now());
        self.document.author_joined(author);

        // Send a ping to the network to ensure that the new author knows we exist
        // Normally we send a ping every `OFFLINE_TIMEOUT / 2`
        if let Err(error) = AuthorMessage::Ping
            .send(&self.tx, &self.node.private_key)
            .await
        {
            error!("Failed to sent ping message to the network: {error}");
        }
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
        if let Err(error) = AuthorMessage::Hello
            .send(&self.tx, &self.node.private_key)
            .await
        {
            error!("Failed to sent hello message to the network: {error}");
        }

        let mut interval = tokio::time::interval(OFFLINE_TIMEOUT / 2);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Skip over the first tick which completes immediately
        interval.tick().await;

        loop {
            interval.tick().await;

            // Send a ping to the network so that we won't be marked as offline
            if let Err(error) = AuthorMessage::Ping
                .send(&self.tx, &self.node.private_key)
                .await
            {
                error!("Failed to sent ping message to the network: {error}");
            }

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
