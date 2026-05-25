//! Peer-to-peer, local-first networking and sync "backend" of Reflection based on p2panda.
//!
//! Some features implemented in `reflection-node` add functionality on top of p2panda:
//!
//! - Author Presence: Indicate which authors have contributed to which topic and if they're
//!   currently online.
//! - Persisted Topics: Store to persist all previously used topics.
mod author_tracker;
mod database;
mod ephemeral_message;
mod migration;
mod node;
mod topic_store;
mod topic_stream;
mod traits;

#[doc(hidden)] // FIXME: We're currently not supporting this feature.
pub use node::ConnectionMode;
pub use node::{Node, NodeError, TrackedAuthor, TrackedTopic};
pub use topic_stream::{PublishError, TopicStream, TopicStreamError};
pub use traits::{TopicSubscription, TopicSubscriptionError};

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use p2panda::{Hash, SigningKey, Topic, VerifyingKey};
    use tokio::sync::{Mutex, mpsc};

    use crate::node::{ConnectionMode, Node};
    use crate::traits::{TopicSubscription, TopicSubscriptionError};

    #[derive(Clone)]
    struct TestDocument {
        tx: mpsc::UnboundedSender<Vec<u8>>,
        rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    }

    impl TestDocument {
        fn new() -> Self {
            let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
            TestDocument {
                tx,
                rx: Arc::new(Mutex::new(rx)),
            }
        }

        async fn wait_for_bytes(&self) -> Vec<u8> {
            self.rx.lock().await.recv().await.unwrap()
        }
    }

    impl TopicSubscription for TestDocument {
        fn bytes_received(&self, _author: VerifyingKey, data: Vec<u8>) {
            self.tx.send(data).unwrap();
        }

        fn author_joined(&self, _author: VerifyingKey) {}
        fn author_left(&self, _author: VerifyingKey) {}
        fn ephemeral_bytes_received(&self, _author: VerifyingKey, _timestamp: u64, _data: Vec<u8>) {
        }
        fn error(&self, _error: TopicSubscriptionError) {}
    }

    #[tokio::test]
    #[test_log::test]
    async fn create_topic() {
        let signing_key = SigningKey::generate();
        let network_id = Hash::digest(b"reflection");
        let node = Node::new(signing_key, network_id, None).await.unwrap();

        let id: [u8; 32] = [0; 32];
        let _sub = node.stream(id, TestDocument::new()).await;
        let topics = node.topics().await.unwrap();

        assert_eq!(topics.len(), 1);
        assert_eq!(topics.first().unwrap().topic, id.into());

        node.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[test_log::test]
    async fn subscribe_topic() {
        let network_id = Hash::digest(b"reflection");
        let topic_id: Topic = [1; 32].into();

        let node = Node::new(SigningKey::generate(), network_id, None)
            .await
            .unwrap();
        node.set_connection_mode(ConnectionMode::Network)
            .await
            .unwrap();

        let test_topic = TestDocument::new();

        let subscription = node.stream(topic_id, test_topic).await.unwrap();

        let node2 = Node::new(SigningKey::generate(), network_id, None)
            .await
            .unwrap();
        node2
            .set_connection_mode(ConnectionMode::Network)
            .await
            .unwrap();

        let test_topic2 = TestDocument::new();

        let _subscription2 = node2.stream(topic_id, test_topic2.clone()).await.unwrap();

        // TODO: Need to sleep here to make sure tx already exists.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        let test_snapshot = "test".as_bytes().to_vec();
        subscription
            .publish_snapshot(test_snapshot.clone())
            .await
            .unwrap();

        assert_eq!(test_topic2.wait_for_bytes().await, test_snapshot);

        node.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
    }
}
