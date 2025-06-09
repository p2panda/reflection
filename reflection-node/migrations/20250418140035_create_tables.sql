CREATE TABLE IF NOT EXISTS authors (
    public_key          TEXT NOT NULL,
    document_id 	TEXT NOT NULL,
    last_seen		INTEGER,
    UNIQUE(public_key, document_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id)
);

CREATE TABLE IF NOT EXISTS documents (
    document_id 	TEXT NOT NULL PRIMARY KEY,
    name		TEXT,
    last_accessed	INTEGER
);