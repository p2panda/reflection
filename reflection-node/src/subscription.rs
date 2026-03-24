use std::mem::take;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use chrono::Utc;
use p2panda::node::CreateStreamError;
use p2panda::streams::{EphemeralStreamPublisher, Offset, StreamEvent, StreamPublisher};
use p2panda_core::Topic;
use thiserror::Error;
use tokio::sync::{RwLock, oneshot};
use tokio::task::{AbortHandle, JoinError};
use tokio_stream::StreamExt;
use tracing::{error, info};

use crate::author_tracker::AuthorTracker;
use crate::message::EphemeralMessage;
use crate::node::NodeInner;
use crate::traits::SubscribableTopic;

#[derive(Debug, Error)]
pub enum SubscriptionError {
    #[error(transparent)]
    Runtime(#[from] JoinError),

    #[error(transparent)]
    TopicStore(#[from] sqlx::Error),

    #[error(transparent)]
    StreamPublish(#[from] p2panda::streams::PublishError),

    #[error(transparent)]
    EphemeralStreamPublish(#[from] p2panda::streams::EphemeralPublishError),

    #[error("streams to publish data into network are not available due to a setup error")]
    BrokenStream,
}

pub struct Subscription<T> {
    inner: Arc<SubscriptionInner<T>>,
    runtime: tokio::runtime::Handle,
    network_monitor_task: AbortHandle,
}

impl<T> Drop for Subscription<T> {
    fn drop(&mut self) {
        self.network_monitor_task.abort();
    }
}

impl<T> Subscription<T>
where
    T: SubscribableTopic + 'static,
{
    pub(crate) async fn new(runtime: tokio::runtime::Handle, inner: SubscriptionInner<T>) -> Self {
        let (ready_tx, ready_rx) = oneshot::channel();

        // Spawn task to establish streams to publish and subscribe to messages, the same task will
        // also await a shutdown signal to drop the streams.
        let inner = Arc::new(inner);
        let inner_clone = inner.clone();
        let network_monitor_task = runtime
            .spawn(async move {
                inner_clone.spawn_network_monitor(ready_tx).await;
            })
            .abort_handle();

        // Wait until streams with network have been established.
        let _ = ready_rx.await;

        Subscription {
            inner,
            runtime,
            network_monitor_task,
        }
    }

    pub async fn publish_delta(&self, data: Vec<u8>) -> Result<(), SubscriptionError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.publish_delta(data).await })
            .await?
    }

    pub async fn publish_snapshot(&self, data: Vec<u8>) -> Result<(), SubscriptionError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.publish_snapshot(data).await })
            .await?
    }

    pub async fn publish_ephemeral(&self, data: Vec<u8>) -> Result<(), SubscriptionError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.publish_ephemeral(data).await })
            .await?
    }

    pub async fn unsubscribe(self) -> Result<(), SubscriptionError> {
        self.network_monitor_task.abort();

        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.unsubscribe().await })
            .await??;

        info!("unsubscribed from topic {}", self.inner.id);

        Ok(())
    }

    /// Set the name for a given topic.
    ///
    /// This information will be written to the database.
    pub async fn set_name(&self, name: Option<String>) -> Result<(), SubscriptionError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.set_name(name).await })
            .await?
    }
}

pub(crate) struct SubscriptionInner<T> {
    tx: RwLock<Option<StreamPublisher<Vec<u8>>>>,
    ephemeral_tx: RwLock<Option<EphemeralStreamPublisher<EphemeralMessage>>>,
    node: Arc<NodeInner>,
    id: Topic,
    subscribable_topic: Arc<T>,
    author_tracker: Arc<AuthorTracker<T>>,
    abort_handles: RwLock<Vec<AbortHandle>>,
}

impl<T> Drop for SubscriptionInner<T> {
    fn drop(&mut self) {
        for handle in self.abort_handles.get_mut() {
            handle.abort();
        }
    }
}

impl<T> SubscriptionInner<T>
where
    T: SubscribableTopic + 'static,
{
    pub fn new(node: Arc<NodeInner>, id: Topic, subscribable_topic: Arc<T>) -> Self {
        let author_tracker = AuthorTracker::new(node.clone(), subscribable_topic.clone());

        SubscriptionInner {
            tx: RwLock::new(None),
            ephemeral_tx: RwLock::new(None),
            node,
            id,
            abort_handles: RwLock::new(Vec::new()),
            subscribable_topic,
            author_tracker,
        }
    }

    pub async fn spawn_network_monitor(&self, ready_signal: oneshot::Sender<()>) {
        // Hold a read lock to the network, so that the network won't be dropped or shutdown.
        let network_guard = self.node.network.read().await;

        let result = setup_streams(
            &self.node,
            network_guard.deref(),
            self.id,
            &self.subscribable_topic,
            &self.author_tracker,
        )
        .await;

        match result {
            Ok((tx, ephemeral_tx, abort_handles)) => {
                *self.tx.write().await = Some(tx);
                *self.ephemeral_tx.write().await = Some(ephemeral_tx);
                *self.abort_handles.write().await = abort_handles;
            }
            Err(error) => {
                self.subscribable_topic.error(error.into());
            }
        }

        drop(network_guard);

        // Inform caller that we're done with setting up the streams. They are ready now to be used
        // for publishing and receiving messages.
        let _ = ready_signal.send(());

        // Wait until we've received signal from node to shut down.
        let shutdown_notification = self.node.shutdown_notifier.notified();
        shutdown_notification.await;

        let _ = self.unsubscribe().await;
    }

    pub async fn unsubscribe(&self) -> Result<(), SubscriptionError> {
        let mut tx_guard = self.tx.write().await;
        let mut ephemeral_tx_guard = self.ephemeral_tx.write().await;
        let mut abort_handles_guard = self.abort_handles.write().await;

        let tx = take(tx_guard.deref_mut());
        let ephemeral_tx = take(ephemeral_tx_guard.deref_mut());
        let abort_handles = take(abort_handles_guard.deref_mut());

        self.node
            .topic_store
            .set_last_accessed_for_topic(&self.id, Some(Utc::now()))
            .await?;

        teardown_streams(
            &self.id,
            &self.author_tracker,
            tx,
            ephemeral_tx,
            abort_handles,
        )
        .await;

        Ok(())
    }

    pub async fn publish_delta(&self, data: Vec<u8>) -> Result<(), SubscriptionError> {
        if let Some(tx) = self.tx.read().await.as_ref() {
            info!("delta operation sent for topic with id {}", self.id);
            tx.publish(data).await?;
        } else {
            return Err(SubscriptionError::BrokenStream);
        }

        Ok(())
    }

    pub async fn publish_snapshot(&self, data: Vec<u8>) -> Result<(), SubscriptionError> {
        if let Some(tx) = self.tx.read().await.as_ref() {
            info!("snapshot saved for topic with id {}", self.id);

            // Append an operation to our log and set the prune flag to true. This will remove
            // previous entries.
            tx.prune(Some(data)).await?;
        } else {
            return Err(SubscriptionError::BrokenStream);
        }

        Ok(())
    }

    pub async fn publish_ephemeral(&self, data: Vec<u8>) -> Result<(), SubscriptionError> {
        if let Some(ephemeral_tx) = self.ephemeral_tx.read().await.as_ref() {
            ephemeral_tx
                .publish(EphemeralMessage::Application(data))
                .await?;
        } else {
            return Err(SubscriptionError::BrokenStream);
        }

        Ok(())
    }

    pub async fn set_name(&self, name: Option<String>) -> Result<(), SubscriptionError> {
        self.node
            .topic_store
            .set_name_for_topic(&self.id, name)
            .await?;

        Ok(())
    }
}

async fn setup_streams<T>(
    node: &Arc<NodeInner>,
    network: &p2panda::Node,
    id: Topic,
    subscribable_topic: &Arc<T>,
    author_tracker: &Arc<AuthorTracker<T>>,
) -> Result<
    (
        StreamPublisher<Vec<u8>>,
        EphemeralStreamPublisher<EphemeralMessage>,
        Vec<AbortHandle>,
    ),
    CreateStreamError,
>
where
    T: SubscribableTopic + 'static,
{
    let mut abort_handles = Vec::with_capacity(3);

    // 1. Handle incoming operations from eventually consistent topic stream.

    // Always start from re-playing _all_ operations in the beginning. This is due to Reflection
    // not keeping materialised document state around and we need to repeat materialising the
    // document at the beginning (in memory). This cost is acceptable since we're frequently
    // pruning the log and the number of operations to process is rather small.
    let offset = Offset::Start;

    let (topic_tx, mut topic_rx) = network.stream_from::<Vec<u8>>(id, offset).await?;

    let node_clone = node.clone();
    let subscribable_topic_clone = subscribable_topic.clone();
    let abort_handle = tokio::spawn(async move {
        while let Some(event) = topic_rx.next().await {
            match event {
                StreamEvent::Processed(operation) => {
                    let author = operation.author();

                    // When we discover a new author we need to add them to our topic store.
                    if let Err(error) = node_clone.topic_store.add_author(&id, &author).await {
                        error!("can't store author to database: {error}");
                    }

                    // Forward the message payload up to the app layer.
                    subscribable_topic_clone.bytes_received(author, operation.message().to_owned());
                }
                StreamEvent::DecodingFailed { error, .. } => {
                    error!("failed decoding incoming operation from stream: {error}");
                }
                StreamEvent::ReplayFailed { error, .. } => {
                    error!("error occurred while replaying operation stream: {error}");
                }
                StreamEvent::SyncStarted { .. } | StreamEvent::SyncEnded { .. } => {
                    // TODO: Handle sync events.
                }
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    // 2. Handle incoming messages from ephemeral topic stream.

    let (ephemeral_tx, mut ephemeral_rx) = network.ephemeral_stream::<EphemeralMessage>(id).await?;

    author_tracker
        .set_topic_tx(Some(ephemeral_tx.clone()))
        .await;

    let author_tracker_clone = author_tracker.clone();
    let subscribable_topic_clone = subscribable_topic.clone();
    let abort_handle = tokio::spawn(async move {
        while let Some(message) = ephemeral_rx.next().await {
            match message.body() {
                EphemeralMessage::Application(bytes) => {
                    subscribable_topic_clone
                        .ephemeral_bytes_received(message.author(), bytes.to_owned());
                }
                EphemeralMessage::AuthorTracker(tracker) => {
                    author_tracker_clone
                        .received(message.author(), tracker.to_owned())
                        .await;
                }
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    // 3. Run task to track online status of authors.

    let author_tracker_clone = author_tracker.clone();
    let abort_handle = tokio::spawn(async move {
        author_tracker_clone.spawn().await;
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    info!("network streams set up for topic {}", id);

    Ok((topic_tx, ephemeral_tx, abort_handles))
}

async fn teardown_streams<T>(
    id: &Topic,
    author_tracker: &Arc<AuthorTracker<T>>,
    tx: Option<StreamPublisher<Vec<u8>>>,
    ephemeral_tx: Option<EphemeralStreamPublisher<EphemeralMessage>>,
    abort_handles: Vec<AbortHandle>,
) where
    T: SubscribableTopic + 'static,
{
    for handle in abort_handles {
        handle.abort();
    }

    author_tracker.set_topic_tx(None).await;

    if tx.is_some() {
        info!("network streams torn down for topic {}", id);
    }

    drop(tx);
    drop(ephemeral_tx);
}
