use std::mem::take;
use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use chrono::Utc;
use p2panda::Topic;
use p2panda::node::CreateStreamError;
use p2panda::streams::{EphemeralStreamPublisher, StreamEvent, StreamFrom, StreamPublisher};
use thiserror::Error;
use tokio::sync::{RwLock, oneshot};
use tokio::task::{AbortHandle, JoinError};
use tokio_stream::StreamExt;
use tracing::{error, info, warn};

use crate::author_tracker::AuthorTracker;
use crate::ephemeral_message::EphemeralMessage;
use crate::node::NodeInner;
use crate::traits::TopicSubscription;

#[derive(Debug, Error)]
pub enum PublishError {
    #[error(transparent)]
    Runtime(#[from] JoinError),

    #[error(transparent)]
    StreamPublish(#[from] p2panda::streams::PublishError),

    #[error(transparent)]
    EphemeralStreamPublish(#[from] p2panda::streams::EphemeralPublishError),

    #[error("streams to publish data into network are not available due to a setup error")]
    BrokenStream,
}

#[derive(Debug, Error)]
pub enum TopicStreamError {
    #[error(transparent)]
    Runtime(#[from] JoinError),

    #[error(transparent)]
    Database(#[from] sqlx::Error),
}

pub struct TopicStream<T> {
    inner: Arc<TopicStreamInner<T>>,
    runtime: tokio::runtime::Handle,
    network_monitor_task: AbortHandle,
}

impl<T> Drop for TopicStream<T> {
    fn drop(&mut self) {
        self.network_monitor_task.abort();
    }
}

impl<T> TopicStream<T>
where
    T: TopicSubscription + 'static,
{
    pub(crate) async fn new(runtime: tokio::runtime::Handle, inner: TopicStreamInner<T>) -> Self {
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

        TopicStream {
            inner,
            runtime,
            network_monitor_task,
        }
    }

    pub async fn publish_delta(&self, data: Vec<u8>) -> Result<(), PublishError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.publish_delta(data).await })
            .await?
    }

    pub async fn publish_snapshot(&self, data: Vec<u8>) -> Result<(), PublishError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.publish_snapshot(data).await })
            .await?
    }

    pub async fn publish_ephemeral(&self, data: Vec<u8>) -> Result<(), PublishError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.publish_ephemeral(data).await })
            .await?
    }

    pub async fn unsubscribe(self) -> Result<(), TopicStreamError> {
        self.network_monitor_task.abort();

        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.unsubscribe().await })
            .await??;

        info!("unsubscribed from topic {}", self.inner.topic);

        Ok(())
    }

    /// Set the name for a given topic.
    ///
    /// This information will be written to the database.
    pub async fn set_name(&self, name: Option<String>) -> Result<(), TopicStreamError> {
        let inner = self.inner.clone();
        self.runtime
            .spawn(async move { inner.set_name(name).await })
            .await?
    }
}

pub(crate) struct TopicStreamInner<T> {
    tx: RwLock<Option<StreamPublisher<Vec<u8>>>>,
    ephemeral_tx: RwLock<Option<EphemeralStreamPublisher<EphemeralMessage>>>,
    node: Arc<NodeInner>,
    topic: Topic,
    subscription: Arc<T>,
    author_tracker: Arc<AuthorTracker<T>>,
    abort_handles: RwLock<Vec<AbortHandle>>,
}

impl<T> Drop for TopicStreamInner<T> {
    fn drop(&mut self) {
        for handle in self.abort_handles.get_mut() {
            handle.abort();
        }
    }
}

impl<T> TopicStreamInner<T>
where
    T: TopicSubscription + 'static,
{
    pub fn new(node: Arc<NodeInner>, topic: Topic, subscription: Arc<T>) -> Self {
        let author_tracker = AuthorTracker::new(node.clone(), subscription.clone());

        TopicStreamInner {
            tx: RwLock::new(None),
            ephemeral_tx: RwLock::new(None),
            node,
            topic,
            abort_handles: RwLock::new(Vec::new()),
            subscription,
            author_tracker,
        }
    }

    pub async fn spawn_network_monitor(&self, ready_signal: oneshot::Sender<()>) {
        // Hold a read lock to the network, so that the network won't be dropped or shutdown.
        let network_guard = self.node.network.read().await;

        let result = setup_streams(
            &self.node,
            network_guard.deref(),
            self.topic,
            &self.subscription,
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
                self.subscription.error(error.into());
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

    pub async fn unsubscribe(&self) -> Result<(), TopicStreamError> {
        let mut tx_guard = self.tx.write().await;
        let mut ephemeral_tx_guard = self.ephemeral_tx.write().await;
        let mut abort_handles_guard = self.abort_handles.write().await;

        let tx = take(tx_guard.deref_mut());
        let ephemeral_tx = take(ephemeral_tx_guard.deref_mut());
        let abort_handles = take(abort_handles_guard.deref_mut());

        self.node
            .topic_store
            .set_last_accessed_for_topic(&self.topic, Some(Utc::now()))
            .await?;

        teardown_streams(
            &self.topic,
            &self.author_tracker,
            tx,
            ephemeral_tx,
            abort_handles,
        )
        .await;

        Ok(())
    }

    pub async fn publish_delta(&self, data: Vec<u8>) -> Result<(), PublishError> {
        if let Some(tx) = self.tx.read().await.as_ref() {
            info!("delta operation sent for topic with id {}", self.topic);
            tx.publish(data).await?;
        } else {
            return Err(PublishError::BrokenStream);
        }

        Ok(())
    }

    pub async fn publish_snapshot(&self, data: Vec<u8>) -> Result<(), PublishError> {
        if let Some(tx) = self.tx.read().await.as_ref() {
            info!("snapshot saved for topic with id {}", self.topic);

            // Append an operation to our log and set the prune flag to true. This will remove
            // previous entries.
            tx.prune(Some(data)).await?;
        } else {
            return Err(PublishError::BrokenStream);
        }

        Ok(())
    }

    pub async fn publish_ephemeral(&self, data: Vec<u8>) -> Result<(), PublishError> {
        if let Some(ephemeral_tx) = self.ephemeral_tx.read().await.as_ref() {
            ephemeral_tx
                .publish(EphemeralMessage::Application(data))
                .await?;
        } else {
            return Err(PublishError::BrokenStream);
        }

        Ok(())
    }

    pub async fn set_name(&self, name: Option<String>) -> Result<(), TopicStreamError> {
        self.node
            .topic_store
            .set_name_for_topic(&self.topic, name)
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
    T: TopicSubscription + 'static,
{
    let mut abort_handles = Vec::with_capacity(3);

    // 1. Handle incoming operations from eventually consistent topic stream.
    // ======================================================================

    // Always start from re-playing _all_ operations in the beginning. This is due to Reflection not
    // keeping materialised document state around and we need to repeat materialising the document
    // at the beginning (in memory).
    //
    // This cost is acceptable since we're frequently pruning the log and the number of operations
    // to process is rather small.
    let stream_from = StreamFrom::Start;

    let (topic_tx, mut topic_rx) = network.stream_from::<Vec<u8>>(id, stream_from).await?;

    let node_clone = node.clone();
    let subscribable_topic_clone = subscribable_topic.clone();
    let abort_handle = tokio::spawn(async move {
        while let Some(event) = topic_rx.next().await {
            match event {
                StreamEvent::Processed { operation, .. } => {
                    let author = operation.author();

                    info!(
                        author = &author.to_string()[0..8],
                        "processed operation with id {}",
                        operation.id()
                    );

                    // When we discover a new author we need to add them to our topic store.
                    if let Err(error) = node_clone.topic_store.add_author(&id, &author).await {
                        error!("can't store author to database: {error}");
                    }

                    // Forward the message payload up to the app layer.
                    subscribable_topic_clone.bytes_received(author, operation.message().to_owned());
                }
                StreamEvent::SyncStarted {
                    remote_node_id,
                    session_id,
                    incoming_operations,
                    outgoing_operations,
                    incoming_bytes,
                    outgoing_bytes,
                    ..
                } => {
                    info!(
                        %session_id,
                        remote_node_id = &remote_node_id.to_string()[0..8],
                        "sync started w. {} incoming ({} bytes) and {} outgoing operations ({} bytes)",
                        incoming_operations,
                        incoming_bytes,
                        outgoing_operations,
                        outgoing_bytes,
                    );
                }
                StreamEvent::SyncEnded {
                    remote_node_id,
                    session_id,
                    error,
                    ..
                } => {
                    match error {
                        Some(error) => {
                            warn!(
                                %session_id,
                                remote_node_id = &remote_node_id.to_string()[0..8],
                                "sync failed with error {error}",
                            );
                        }
                        None => {
                            info!(
                                %session_id,
                                remote_node_id = &remote_node_id.to_string()[0..8],
                                "sync ended",
                            );
                        },
                    }
                }
                StreamEvent::DecodeFailed { error, .. } => {
                    error!("failed decoding incoming operation from stream: {error}");
                    subscribable_topic_clone.error(error.into());
                }
                StreamEvent::ReplayFailed { error, .. } => {
                    error!("error occurred while replaying operation stream: {error}");
                    subscribable_topic_clone.error(crate::traits::TopicSubscriptionError::ReplayStream(error));
                }
                StreamEvent::ProcessingFailed { event, error, .. } => {
                    error!("error occurred while processing operation {}: {error}", event.header().hash());
                    subscribable_topic_clone.error(error.into());
                }
                StreamEvent::AckFailed { error, .. } => {
                    error!("error occurred while acking event: {error}");
                    subscribable_topic_clone.error(crate::traits::TopicSubscriptionError::AckedMessage(error));
                }
                _ => (),
            }
        }
    })
    .abort_handle();

    abort_handles.push(abort_handle);

    // 2. Handle incoming messages from ephemeral topic stream.
    // ========================================================

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
                    subscribable_topic_clone.ephemeral_bytes_received(
                        message.author(),
                        message.timestamp(),
                        bytes.to_owned(),
                    );
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
    // ==============================================

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
    topic: &Topic,
    author_tracker: &Arc<AuthorTracker<T>>,
    tx: Option<StreamPublisher<Vec<u8>>>,
    ephemeral_tx: Option<EphemeralStreamPublisher<EphemeralMessage>>,
    abort_handles: Vec<AbortHandle>,
) where
    T: TopicSubscription + 'static,
{
    for handle in abort_handles {
        handle.abort();
    }

    author_tracker.set_topic_tx(None).await;

    if tx.is_some() {
        info!("network streams torn down for topic {}", topic);
    }

    drop(tx);
    drop(ephemeral_tx);
}
