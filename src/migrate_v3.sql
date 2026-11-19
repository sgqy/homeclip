-- Run in a transaction with foreign keys disabled while rebuilding files.
CREATE TABLE blobs (
    id TEXT PRIMARY KEY,
    sha256 TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size >= 0),
    UNIQUE (sha256, size)
);
INSERT INTO blobs(id, sha256, size)
    SELECT MIN(id), sha256, size FROM files WHERE sha256 IS NOT NULL GROUP BY sha256, size;

CREATE TABLE message_files (
    message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    file_id TEXT NOT NULL REFERENCES files(id),
    position INTEGER NOT NULL,
    PRIMARY KEY (message_id, file_id)
);
INSERT INTO message_files(message_id, file_id, position)
    SELECT message_id, id, rowid FROM files WHERE message_id IS NOT NULL;

CREATE TABLE files_v3 (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size >= 0),
    mime TEXT NOT NULL,
    received INTEGER NOT NULL DEFAULT 0 CHECK (received >= 0 AND received <= size),
    sha256 TEXT,
    expected_sha256 TEXT,
    blob_id TEXT REFERENCES blobs(id),
    created_at INTEGER NOT NULL,
    CHECK ((sha256 IS NULL) = (blob_id IS NULL))
);
INSERT INTO files_v3(id, name, size, mime, received, sha256, expected_sha256, blob_id, created_at)
    SELECT f.id, f.name, f.size, f.mime, f.received, f.sha256, f.sha256, b.id, f.created_at
    FROM files f LEFT JOIN blobs b ON b.sha256 = f.sha256 AND b.size = f.size;
INSERT OR IGNORE INTO garbage(id)
    SELECT id FROM files_v3 WHERE blob_id IS NOT NULL AND id != blob_id;
DROP TABLE files;
ALTER TABLE files_v3 RENAME TO files;
PRAGMA user_version = 3;
