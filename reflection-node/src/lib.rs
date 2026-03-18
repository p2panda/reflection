mod author_tracker;
mod database;
mod message;
pub mod node;
pub mod subscription;
mod topic_store;
pub mod traits;

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use p2panda_core::{Hash, SigningKey, Topic, VerifyingKey};
    use tokio::sync::{Mutex, mpsc};

    use crate::node::ConnectionMode;
    use crate::node::Node;
    use crate::topic::SubscribableTopic;

    #[tokio::test]
    #[test_log::test]
    async fn create_topic() {
        let signing_key = SigningKey::generate();
        let network_id = Hash::digest(b"reflection");
        let node = Node::new(signing_key, network_id, None).await.unwrap();

        let id: [u8; 32] = [0; 32];
        let _sub = node.subscribe(id, TestTopic::new()).await;
        let topics = node.topics::<[u8; 32]>().await.unwrap();

        assert_eq!(topics.len(), 1);
        assert_eq!(topics.first().unwrap().id, id);

        node.shutdown().await.unwrap();
    }

    #[derive(Clone)]
    struct TestTopic {
        tx: mpsc::UnboundedSender<Vec<u8>>,
        rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<u8>>>>,
    }

    impl TestTopic {
        fn new() -> Self {
            let (tx, rx) = mpsc::unbounded_channel::<Vec<u8>>();
            TestTopic {
                tx,
                rx: Arc::new(Mutex::new(rx)),
            }
        }

        async fn wait_for_bytes(&self) -> Vec<u8> {
            self.rx.lock().await.recv().await.unwrap()
        }
    }

    impl SubscribableTopic for TestTopic {
        fn bytes_received(&self, _author: VerifyingKey, data: Vec<u8>) {
            self.tx.send(data).unwrap();
        }

        fn author_joined(&self, _author: VerifyingKey) {}
        fn author_left(&self, _author: VerifyingKey) {}
        fn ephemeral_bytes_received(&self, _author: VerifyingKey, _data: Vec<u8>) {}
        fn error(&self, _error: SubscriptionError) {}
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

        let test_topic = TestTopic::new();

        let id: [u8; 32] = [0; 32];
        let subscription = node.subscribe(id, test_topic).await.unwrap();

        let topics = node.topics::<[u8; 32]>().await.unwrap();
        assert_eq!(topics.len(), 1);
        assert_eq!(topics.first().unwrap().id, id);

        let private_key2 = PrivateKey::new();
        let network_id2 = Hash::new(b"reflection");
        let node2 = Node::new(private_key2, network_id2, None).await.unwrap();
        node2
            .set_connection_mode(ConnectionMode::Network)
            .await
            .unwrap();

        let test_topic2 = TestTopic::new();

        let _subscription2 = node2.subscribe(id, test_topic2.clone()).await.unwrap();

        let topics2 = node2.topics::<[u8; 32]>().await.unwrap();
        assert_eq!(topics2.len(), 1);
        assert_eq!(topics2.first().unwrap().id, id);

        let test_snapshot = "test".as_bytes().to_vec();
        subscription
            .send_snapshot(test_snapshot.clone())
            .await
            .unwrap();

        assert_eq!(test_topic2.wait_for_bytes().await, test_snapshot);

        node.shutdown().await.unwrap();
        node2.shutdown().await.unwrap();
    }
}
