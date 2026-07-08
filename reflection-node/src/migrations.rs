add `_reflection_migrations` table to store version, checksum, execution time, filename

struct Migrations {};

 #[derive(Debug, FromRow)]
 struct Operation {
     #[sqlx(rename = "log_id")]
     pub log_id_hash: String,
     // FIXME:  needs to be hex decoded hex::decode() and we need to hope that the public key format didn't change
     //pub public_key: String,
     pub body: Option<Vec<u8>>,
 }

#[derive(Clone, Debug, PartialEq, Eq, StdHash, Serialize, Deserialize)]
pub struct LogId(LogType, TopicId);

impl LogId {
	pub fn new(log_type: LogType, topic: &TopicId) -> Self {
	 	Self(log_type, *topic)
	}

	pub fn hash(&mut self) -> u64 {
	    let mut s = DefaultHasher::new();
		t.hash(&mut s);
    		s.finish()
	}
}

 PublicKey::from_str(&row.public_key).unwrap(),


impl Migrations {
let connection_options = sqlx::sqlite::SqliteConnectOptions::new()
            .shared_cache(true)
            .create_if_missing(true);

            let connection_options = connection_options.filename(db_file);
     let db_file = sqlx::sqlite::SqlitePool::connect_with(connection_options).await?

    async fn load_operations(
        &self,
        public_key: &PublicKey,
    ) -> Result<Vec<Operation>, SqliteStoreError> {
        let operations = query_as::<_, Operation>(
            "
            SELECT
                log_id,
                body,
            FROM
                operations_v1
             GROUP_BY
                log_id
            ",
        )
        .bind(public_key.to_string())
        .fetch_all(&self.pool)
        .await?;
    }


    fn calculate_hash<T: StdHash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}

pub async fn operations_for_topic(
        &self,
        operation_store: &OperationStore,
        id: &TopicId,
    ) -> sqlx::Result<Vec<(PublicKey, Option<Vec<u8>>)>> {
        let topics = todo!("get all topics");

	topcis.map(|id|
		(id,
            LogId::new(LogType::Delta, id).hash(),
            LogId::new(LogType::Snapshot, id).hash(),
        ));

        let operations = self.load_operations();

        Ok(result)
    }


    /*
        async fn get_log(
        &self,
        public_key: &PublicKey,
        log_id: &LogId,
        from: Option<u64>,
    ) -> Result<Option<Vec<(Header<Extensions>, Option<Body>)>>, SqliteStoreError> {
        let operations = query_as::<_, OperationRow>(
            "
            SELECT
                hash,
                log_id,
                version,
                public_key,
                signature,
                payload_size,
                payload_hash,
                timestamp,
                seq_num,
                backlink,
                previous,
                extensions,
                body,
                header_bytes
            FROM
                operations_v1
            WHERE
                public_key = ?
                AND log_id = ?
                AND CAST(seq_num AS NUMERIC) >= CAST(? as NUMERIC)
            ORDER BY
                CAST(seq_num AS NUMERIC)
            ",
        )
        .bind(public_key.to_string())
        .bind(calculate_hash(log_id).to_string())
        .bind(from.unwrap_or(0).to_string())
        .fetch_all(&self.pool)
        .await?;

        let log: Vec<(Header<E>, Option<Body>)> = operations
            .into_iter()
            .map(|operation| {
                (
                    operation.clone().into(),
                    operation.body.map(|body| body.into()),
                )
            })
            .collect();

        if log.is_empty() {
            Ok(None)
        } else {
            Ok(Some(log))
        }
    }
    */