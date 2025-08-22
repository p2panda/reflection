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
    use std::sync::Arc;
    use test_log::test;
    use tokio::sync::{Mutex, mpsc};

    #[test]
    fn create_document() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .unwrap();

        let node = runtime.block_on(async move {
            let private_key = PrivateKey::new();
            let network_id = Hash::new(b"reflection");
            let node = Node::new();
            node.run(private_key, network_id, None).await.unwrap();

            let document_id = node.create_document().await.unwrap();
            let documents = node.documents().await.unwrap();

            assert_eq!(documents.len(), 1);
            assert_eq!(documents.first().unwrap().id, document_id);

            node.shutdown().await.unwrap();
            node
        });

        // Node can't be dropped inside an async context
        drop(node);
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
            self.rx.lock().await.recv().await.unwrap()
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

    #[test]
    fn subscribe_document() {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .build()
            .unwrap();

        let nodes = runtime.block_on(async move {
            let private_key = PrivateKey::new();
            let network_id = Hash::new(b"reflection");
            let node = Node::new();
            node.run(private_key, network_id, None).await.unwrap();

            let test_document = TestDocument::new();

            let document_id = node.create_document().await.unwrap();
            let documents = node.documents().await.unwrap();
            assert_eq!(documents.len(), 1);
            assert_eq!(documents.first().unwrap().id, document_id);

            let subscription = node.subscribe(document_id, test_document).await.unwrap();

            let document_id = subscription.id;

            let private_key2 = PrivateKey::new();
            let network_id2 = Hash::new(b"reflection");
            let node2 = Node::new();
            node2.run(private_key2, network_id2, None).await.unwrap();

            let test_document2 = TestDocument::new();

            let _subscription2 = node2
                .subscribe(document_id, test_document2.clone())
                .await
                .unwrap();

            let documents2 = node2.documents().await.unwrap();
            assert_eq!(documents2.len(), 1);
            assert_eq!(documents2.first().unwrap().id, document_id);

            let test_snapshot = "test".as_bytes().to_vec();
            subscription
                .send_snapshot(test_snapshot.clone())
                .await
                .unwrap();

            assert_eq!(test_document2.wait_for_bytes().await, test_snapshot);

            node.shutdown().await.unwrap();
            node2.shutdown().await.unwrap();

            (node, node2)
        });

        // Node can't be dropped inside a tokio async context
        drop(nodes);
    }
}
