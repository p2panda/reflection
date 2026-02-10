mod author_tracker;
mod ephemerial_operation;
mod network;
pub mod node;
mod node_inner;
mod operation;
mod operation_store;
mod subscription_inner;
pub mod topic;
mod topic_store;
mod utils;

pub use p2panda_core;
pub use topic::SubscribableTopic;

#[cfg(test)]
mod tests {
    use crate::SubscribableTopic;
    use crate::node::{ConnectionMode, Node};
    use p2panda_core::Hash;
    use p2panda_core::PrivateKey;
    use p2panda_core::PublicKey;
    use std::sync::Arc;
    use tokio::sync::{Mutex, mpsc};

    #[tokio::test]
    #[test_log::test]
    async fn create_topic() {
        let private_key = PrivateKey::new();
        let network_id = Hash::new(b"reflection");
        let node = Node::new(private_key, network_id, None, ConnectionMode::Network)
            .await
            .unwrap();

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
        fn bytes_received(&self, _author: PublicKey, data: Vec<u8>) {
            self.tx.send(data).unwrap();
        }

        fn author_joined(&self, _author: PublicKey) {}
        fn author_left(&self, _author: PublicKey) {}
        fn ephemeral_bytes_received(&self, _author: PublicKey, _data: Vec<u8>) {}
    }

    #[tokio::test]
    #[test_log::test]
    async fn subscribe_topic() {
        let private_key = PrivateKey::new();
        let network_id = Hash::new(b"reflection");
        let node = Node::new(private_key, network_id, None, ConnectionMode::Network)
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
        let node2 = Node::new(private_key2, network_id2, None, ConnectionMode::Network)
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
