CREATE TABLE IF NOT EXISTS authors (
    public_key          TEXT NOT NULL,
    topic_id          	TEXT NOT NULL,
    last_seen		INTEGER,
    UNIQUE(public_key, topic_id),
    FOREIGN KEY(topic_id) REFERENCES topics(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS topics (
    id	 	        TEXT NOT NULL PRIMARY KEY,
    name		TEXT,
    last_accessed	INTEGER
);