use std::path::Path;
use std::sync::Arc;

use crate::author_tracker::{AuthorMessage, AuthorTracker};
use crate::document::{DocumentError, DocumentId, SubscribableDocument, Subscription};
use crate::document_store::DocumentStore;
use crate::ephemerial_operation::EphemerialOperation;
use crate::node::NodeError;
use crate::operation::{LogType, ReflectionExtensions};
use crate::operation_store::OperationStore;
use crate::persistent_operation::PersistentOperation;
use crate::utils::CombinedMigrationSource;

use p2panda_core::{Body, Hash, Header, PrivateKey, cbor::decode_cbor};
use p2panda_discovery::mdns::LocalDiscovery;
use p2panda_net::config::GossipConfig;
use p2panda_net::{FromNetwork, NetworkBuilder, SyncConfiguration};
use p2panda_store::sqlite::store::migrations as operation_store_migrations;
use p2panda_stream::IngestExt;
use p2panda_sync::log_sync::LogSyncProtocol;
use sqlx::{migrate::Migrator, sqlite};
use tokio::{
    runtime::{Builder, Runtime},
    sync::mpsc,
};
use tokio_stream::{StreamExt, wrappers::ReceiverStream};
use tracing::{error, info, warn};

#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub(crate) enum MessageType {
    Persistent(PersistentOperation),
    Ephemeral(EphemerialOperation),
    AuthorEphemeral(EphemerialOperation),
}

#[derive(Debug)]
pub struct NodeInner {
    pub(crate) runtime: Runtime,
    pub(crate) operation_store: OperationStore,
    pub(crate) document_store: DocumentStore,
    pub(crate) private_key: PrivateKey,
    pub(crate) network: p2panda_net::Network<DocumentId>,
}

//const RELAY_URL: &str = "https://staging-euw1-1.relay.iroh.network/";
//const BOOTSTRAP_NODE_ID: &str = "d825a2f929f935efcd6889bed5c3f5510b40f014969a729033d3fb7e33b97dbe";

impl NodeInner {
    pub async fn new(
        network_id: Hash,
        private_key: PrivateKey,
        db_location: Option<&Path>,
    ) -> Result<Self, NodeError> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()?;

        let _guard = runtime.enter();

        let connection_options = sqlx::sqlite::SqliteConnectOptions::new()
            .shared_cache(true)
            .create_if_missing(true);
        let connection_options = if let Some(db_location) = db_location {
            let db_file = db_location.join("database.sqlite");
            info!("Database file location: {db_file:?}");
            connection_options.filename(db_file)
        } else {
            connection_options.in_memory(true)
        };

        let pool = if db_location.is_some() {
            sqlx::sqlite::SqlitePool::connect_with(connection_options).await?
        } else {
            // FIXME: we need to set max connection to 1 for in memory sqlite DB.
            // Probably has to do something with this issue: https://github.com/launchbadge/sqlx/issues/2510
            let pool_options = sqlite::SqlitePoolOptions::new().max_connections(1);
            pool_options.connect_with(connection_options).await?
        };

        // Run migration for p2panda OperationStore and for the our DocumentStore
        Migrator::new(CombinedMigrationSource::new(vec![
            operation_store_migrations(),
            sqlx::migrate!(),
        ]))
        .await?
        .run(&pool)
        .await?;

        let operation_store = OperationStore::new(pool.clone());
        let document_store = DocumentStore::new(pool);

        let sync_config = {
            let sync = LogSyncProtocol::new(document_store.clone(), operation_store.clone_inner());
            SyncConfiguration::<DocumentId>::new(sync)
        };

        let network = NetworkBuilder::new(network_id.into())
            .private_key(private_key.clone())
            .discovery(LocalDiscovery::new())
            // NOTE(glyph): Internet networking is disabled until we can fix the
            // more-than-two-peers gossip issue.
            //
            //.relay(RELAY_URL.parse().expect("valid relay URL"), false, 0)
            //.direct_address(
            //    BOOTSTRAP_NODE_ID.parse().expect("valid node ID"),
            //    vec![],
            //    None,
            //)
            .gossip(GossipConfig {
                // FIXME: This is a temporary workaround to account for larger delta patches (for
                // example when the user Copy & Pastes a big chunk of text).
                //
                // Related issue: https://github.com/p2panda/reflection/issues/24
                max_message_size: 512_000,
            })
            .sync(sync_config)
            .build()
            .await?;

        Ok(Self {
            runtime,
            operation_store,
            document_store,
            private_key,
            network,
        })
    }

    pub async fn shutdown(&self) -> Result<(), NodeError> {
        // FIXME: If we can just clone the network why does shutdown consume self?
        self.network.clone().shutdown().await?;
        Ok(())
    }

    pub async fn create_document(self: Arc<Self>) -> Result<DocumentId, DocumentError> {
        let operation = self
            .operation_store
            .create_operation(&self.private_key, LogType::Snapshot, None, None, false)
            .await?;

        let document_id: DocumentId = operation
            .header
            .extension()
            .expect("document id from our own logs");
        self.document_store.add_document(&document_id).await?;

        // Add ourselves as an author to the document store.
        self.document_store
            .add_author(&document_id, &self.private_key.public_key())
            .await?;
        Ok(document_id)
    }

    pub async fn subscribe(
        self: Arc<Self>,
        document_id: DocumentId,
        document: Arc<impl SubscribableDocument + 'static>,
    ) -> Result<Subscription, DocumentError> {
        self.document_store.add_document(&document_id).await?;
        // Add ourselves as an author to the document store.
        self.document_store
            .add_author(&document_id, &self.private_key.public_key())
            .await?;
        let stored_operations = self
            .document_store
            .operations_for_document(&self.operation_store, &document_id)
            .await?;

        for operation in stored_operations {
            // Send all stored operation bytes to the app,
            // it doesn't matter if the app already knows some or all of them
            if let Some(body) = operation.body {
                document.bytes_received(operation.header.public_key, body.to_bytes());
            }
        }

        // Join a gossip overlay with peers who are interested in the same document and start sync
        // with them.
        let (document_tx, mut document_rx, gossip_ready) =
            self.network.subscribe(document_id).await?;

        let (persistent_tx, persistent_rx) =
            mpsc::channel::<(Header<ReflectionExtensions>, Option<Body>, Vec<u8>)>(128);

        let author_tracker =
            AuthorTracker::new(self.clone(), document.clone(), document_tx.clone());

        let author_tracker_clone = author_tracker.clone();
        let document_clone = document.clone();
        let subscription_abort_handle = self
            .runtime
            .spawn(async move {
                while let Some(event) = document_rx.recv().await {
                    match event {
                        FromNetwork::GossipMessage { bytes, .. } => match decode_cbor(&bytes[..]) {
                            Ok(MessageType::Ephemeral(operation)) => {
                                if let Some((author, body)) = operation.validate_and_unpack() {
                                    document_clone.ephemeral_bytes_received(author, body);
                                } else {
                                    warn!("Got ephemeral operation with a bad signature");
                                }
                            }
                            Ok(MessageType::AuthorEphemeral(operation)) => {
                                if let Some((author, body)) = operation.validate_and_unpack() {
                                    match AuthorMessage::try_from(&body[..]) {
                                        Ok(message) => {
                                            author_tracker_clone.received(message, author).await;
                                        }
                                        Err(error) => {
                                            warn!("Failed to deserialize AuthorMessage: {error}");
                                        }
                                    }
                                } else {
                                    warn!("Got internal ephemeral operation with a bad signature");
                                }
                            }
                            Ok(MessageType::Persistent(operation)) => {
                                match operation.validate_and_unpack(document_id) {
                                    Ok(data) => {
                                        persistent_tx.send(data).await.unwrap();
                                    }
                                    Err(err) => {
                                        error!("Failed to unpack operation: {err}");
                                    }
                                }
                            }
                            Err(err) => {
                                error!("Failed to decode gossip message: {err}");
                            }
                        },
                        FromNetwork::SyncMessage {
                            header, payload, ..
                        } => match PersistentOperation::from_serialized(header, payload)
                            .validate_and_unpack(document_id)
                        {
                            Ok(data) => persistent_tx.send(data).await.unwrap(),
                            Err(err) => {
                                error!("Failed to unpack operation: {err}");
                            }
                        },
                    }
                }
            })
            .abort_handle();

        let stream = ReceiverStream::new(persistent_rx);

        // Ingest does multiple things for us:
        //
        // - Validate operation- and log integrity and authenticity
        // - De-duplicate already known operations
        // - Out-of-order buffering
        // - Pruning when flag is set
        // - Persist operation in store
        let mut stream = stream
            // NOTE(adz): The persisting part should happen later, we want to check the payload on
            // application layer first. In general "ingest" does too much at once and is
            // inflexible. Related issue: https://github.com/p2panda/p2panda/issues/696
            .ingest(self.operation_store.clone_inner(), 128)
            .filter_map(|result| match result {
                Ok(operation) => Some(operation),
                Err(err) => {
                    error!("ingesting operation failed: {err}");
                    None
                }
            });

        let inner_clone = self.clone();
        let document_clone = document.clone();
        // Send checked and ingested operations for this document to application layer.
        let subscription2_abort_handle = self
            .runtime
            .spawn(async move {
                while let Some(operation) = stream.next().await {
                    // When we discover a new author we need to add them to our document store.
                    if let Err(error) = inner_clone
                        .document_store
                        .add_author(&document_id, &operation.header.public_key)
                        .await
                    {
                        error!("Can't store author to database: {error}");
                    }

                    // Forward the payload up to the app.
                    if let Some(body) = operation.body {
                        document_clone.bytes_received(operation.header.public_key, body.to_bytes());
                    }
                }
            })
            .abort_handle();

        let author_tracker_abort_handle = self
            .runtime
            .spawn(async move {
                // Only start track authors once we have joined the gossip overlay
                if let Err(error) = gossip_ready.await {
                    error!("Failed to join the gossip overlay: {error}");
                }
                author_tracker.spawn().await;
            })
            .abort_handle();

        info!("Subscribed to document {document_id}");

        Ok(Subscription {
            tx: document_tx,
            id: document_id,
            node: self,
            abort_handles: vec![
                author_tracker_abort_handle,
                subscription_abort_handle,
                subscription2_abort_handle,
            ],
        })
    }
}
