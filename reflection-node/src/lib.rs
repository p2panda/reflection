pub mod document;
mod ephemerial_operation;
mod node;
mod node_inner;
mod operation;
mod operation_store;
mod persistent_operation;
mod store;
mod utils;

pub use document::SubscribableDocument;
pub use node::Node;
pub use p2panda_core;

#[cfg(test)]
mod tests {
    use crate::Node;
    use crate::SubscribableDocument;
    use p2panda_core::Hash;
    use p2panda_core::PrivateKey;
    use p2panda_core::PublicKey;
    use std::sync::{Arc, Mutex};
    use test_log::test;
    use tokio::sync::mpsc;

    #[test(tokio::test(flavor = "multi_thread", worker_threads = 1))]
    async fn create_document() {
        let node = Node::new();
        let private_key = PrivateKey::new();
        let network_id = Hash::new(b"reflection");
        node.run(private_key, network_id, None).await.unwrap();
        let document_id = node.create_document().await.unwrap();
        let documents = node.documents().await.unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents.first().unwrap().id, document_id);
    }
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
            self.rx.lock().unwrap().recv().await.unwrap()
        }
    }

    impl SubscribableDocument for TestDocument {
        fn bytes_received(&self, _author: PublicKey, data: Vec<u8>) {
            self.tx.send(data).unwrap();
        }

        fn authors_joined(&self, _authors: Vec<PublicKey>) {}
        fn author_set_online(&self, _author: PublicKey, _is_online: bool) {}
        fn ephemeral_bytes_received(&self, _author: PublicKey, _data: Vec<u8>) {}
    }

    #[test(tokio::test(flavor = "multi_thread", worker_threads = 1))]
    async fn subscribe_document() {
        let node = Node::new();
        let test_document = TestDocument::new();
        let private_key = PrivateKey::new();
        let network_id = Hash::new(b"reflection");
        node.run(private_key, network_id, None).await.unwrap();
        let document_id = node.create_document().await.unwrap();
        let documents = node.documents().await.unwrap();
        assert_eq!(documents.len(), 1);
        assert_eq!(documents.first().unwrap().id, document_id);

        node.subscribe(document_id, test_document).await.unwrap();

        let node2 = Node::new();
        let test_document2 = TestDocument::new();
        let private_key = PrivateKey::new();
        let network_id = Hash::new(b"reflection");
        node2.run(private_key, network_id, None).await.unwrap();
        node2
            .subscribe(document_id, test_document2.clone())
            .await
            .unwrap();
        let documents2 = node2.documents().await.unwrap();
        assert_eq!(documents2.len(), 1);
        assert_eq!(documents2.first().unwrap().id, document_id);

        let test_snapshot = "test".as_bytes().to_vec();
        node.snapshot(document_id, test_snapshot.clone())
            .await
            .unwrap();

        assert_eq!(test_document2.wait_for_bytes().await, test_snapshot);
    }
}
