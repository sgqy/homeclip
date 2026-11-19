PRAGMA journal_mode = WAL;
PRAGMA synchronous = FULL;
PRAGMA foreign_keys = ON;
PRAGMA cache_size = -4096;

CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id TEXT NOT NULL UNIQUE,
    text TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    reply_to INTEGER REFERENCES messages(id) ON DELETE SET NULL
);

CREATE TABLE IF NOT EXISTS files (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    size INTEGER NOT NULL CHECK (size >= 0),
    mime TEXT NOT NULL,
    received INTEGER NOT NULL DEFAULT 0 CHECK (received >= 0 AND received <= size),
    sha256 TEXT,
    created_at INTEGER NOT NULL,
    message_id INTEGER REFERENCES messages(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS files_message ON files(message_id);

-- Keep only ranges beyond the contiguous prefix. Merge and delete them as gaps close.
CREATE TABLE IF NOT EXISTS upload_parts (
    file_id TEXT NOT NULL REFERENCES files(id) ON DELETE CASCADE,
    offset INTEGER NOT NULL CHECK (offset >= 0),
    length INTEGER NOT NULL CHECK (length > 0),
    PRIMARY KEY (file_id, offset)
);

CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    message_id INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS garbage (id TEXT PRIMARY KEY);
PRAGMA user_version = 2;
