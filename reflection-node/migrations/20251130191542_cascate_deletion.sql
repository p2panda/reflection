CREATE TABLE IF NOT EXISTS new_authors (
    public_key          TEXT NOT NULL,
    document_id 	TEXT NOT NULL,
    last_seen		INTEGER,
    UNIQUE(public_key, document_id),
    FOREIGN KEY(document_id) REFERENCES documents(document_id) ON DELETE CASCADE
);

INSERT INTO new_authors SELECT * FROM authors;

DROP TABLE authors;

ALTER TABLE new_authors RENAME TO authors;