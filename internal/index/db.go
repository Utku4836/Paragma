// Package index maintains the SQLite index of a vault. Markdown files are
// the source of truth; the index is disposable and rebuilt on demand.
package index

import (
	"database/sql"
	"fmt"
	"os"

	"github.com/Utku-Private/Paragma/internal/vault"
	_ "modernc.org/sqlite"
)

// schemaVersion is bumped whenever the schema or the parser output changes;
// a mismatch triggers a full rebuild.
const schemaVersion = 8

const schema = `
CREATE TABLE files (
  id INTEGER PRIMARY KEY,
  path TEXT UNIQUE NOT NULL,
  ulid TEXT,
  type TEXT, layer TEXT, status TEXT, title TEXT,
  mtime INTEGER, size INTEGER, hash TEXT,
  created TEXT, updated TEXT, verified TEXT,
  seen INTEGER, tags TEXT, tokens INTEGER, lines INTEGER, body_line INTEGER, branch TEXT, fm_json TEXT, fm_error TEXT
);
CREATE INDEX files_ulid ON files(ulid);

CREATE TABLE sections (
  id INTEGER PRIMARY KEY,
  file_id INTEGER NOT NULL,
  ord INTEGER,
  heading_path TEXT,
  level INTEGER,
  line_start INTEGER, body_end INTEGER, line_end INTEGER,
  tokens INTEGER, sub_tokens INTEGER,
  body TEXT, kinds TEXT, tags TEXT, is_title INTEGER
);
CREATE INDEX sections_file ON sections(file_id);

CREATE VIRTUAL TABLE sections_fts USING fts5(
  heading_path, body, tags,
  content='sections', content_rowid='id',
  tokenize='unicode61 remove_diacritics 2'
);
CREATE VIRTUAL TABLE sections_tri USING fts5(
  body, content='sections', content_rowid='id', tokenize='trigram'
);
CREATE TRIGGER sections_ai AFTER INSERT ON sections BEGIN
  INSERT INTO sections_fts(rowid, heading_path, body, tags) VALUES (new.id, new.heading_path, new.body, new.tags);
  INSERT INTO sections_tri(rowid, body) VALUES (new.id, new.body);
END;
CREATE TRIGGER sections_ad AFTER DELETE ON sections BEGIN
  INSERT INTO sections_fts(sections_fts, rowid, heading_path, body, tags) VALUES ('delete', old.id, old.heading_path, old.body, old.tags);
  INSERT INTO sections_tri(sections_tri, rowid, body) VALUES ('delete', old.id, old.body);
END;

CREATE TABLE blocks (
  file_id INTEGER, block_id TEXT, section_id INTEGER, line INTEGER, line_end INTEGER,
  PRIMARY KEY (file_id, block_id)
);
CREATE INDEX blocks_id ON blocks(block_id);

CREATE TABLE links (
  src_file INTEGER, src_section INTEGER, line INTEGER,
  target TEXT, target_file TEXT, target_sub TEXT, kind TEXT, context TEXT
);
CREATE INDEX links_src ON links(src_file);
CREATE INDEX links_tf ON links(target_file);

CREATE TABLE tasks (
  file_id INTEGER, section_id INTEGER, block_id TEXT, parent TEXT,
  line INTEGER, line_end INTEGER, state TEXT, text TEXT,
  priority TEXT, due TEXT, tags TEXT, done_at TEXT, blocked_by TEXT, claimed_by TEXT
);
CREATE INDEX tasks_file ON tasks(file_id);
CREATE INDEX tasks_block ON tasks(block_id);

CREATE TABLE marks (file_id INTEGER, section_id INTEGER, block_id TEXT, line INTEGER, kind TEXT, note TEXT, created TEXT);
CREATE INDEX marks_file ON marks(file_id);

CREATE TABLE tags (file_id INTEGER, tag TEXT);
CREATE INDEX tags_tag ON tags(tag);
CREATE INDEX tags_file ON tags(file_id);

CREATE TABLE error_sigs (file_id INTEGER, sig_norm TEXT, sig_hash TEXT);
CREATE INDEX error_sigs_hash ON error_sigs(sig_hash);

CREATE TABLE code_refs (file_id INTEGER, section_id INTEGER, line INTEGER, ref TEXT);
CREATE INDEX code_refs_ref ON code_refs(ref);
CREATE INDEX code_refs_file ON code_refs(file_id);
`

// DB is an open index for one vault.
type DB struct {
	*sql.DB
	V *vault.Vault

	files []*File // cache, see Files
	byID  map[int64]*File
}

func dsn(path string) string {
	return path + "?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)&_pragma=synchronous(NORMAL)&_pragma=mmap_size(268435456)&_txlock=immediate"
}

// Open opens (creating or rebuilding as needed) the index of v. It does not
// sync; call Sync.
func Open(v *vault.Vault) (*DB, error) {
	db, err := sql.Open("sqlite", dsn(v.DBPath()))
	if err != nil {
		return nil, err
	}
	db.SetMaxOpenConns(1)
	d := &DB{DB: db, V: v}
	var ver int
	if err := db.QueryRow("PRAGMA user_version").Scan(&ver); err != nil {
		db.Close()
		return nil, fmt.Errorf("open index: %w", err)
	}
	if ver != 0 && ver != schemaVersion {
		// older or newer layout: the index is disposable, rebuild from scratch
		db.Close()
		return Reset(v)
	}
	if ver == 0 {
		if err := d.create(); err != nil {
			db.Close()
			return nil, err
		}
	}
	return d, nil
}

// Reset deletes the database file and recreates an empty schema.
func Reset(v *vault.Vault) (*DB, error) {
	for _, suf := range []string{"", "-wal", "-shm"} {
		if err := os.Remove(v.DBPath() + suf); err != nil && !os.IsNotExist(err) {
			return nil, fmt.Errorf("remove index: %w", err)
		}
	}
	return Open(v)
}

func (d *DB) create() error {
	tx, err := d.Begin()
	if err != nil {
		return err
	}
	defer tx.Rollback()
	// another pg process may have created the schema while we waited for the lock
	var ver int
	if err := tx.QueryRow("PRAGMA user_version").Scan(&ver); err == nil && ver == schemaVersion {
		return nil
	}
	if _, err := tx.Exec(schema); err != nil {
		return fmt.Errorf("create schema: %w", err)
	}
	if _, err := tx.Exec(fmt.Sprintf("PRAGMA user_version = %d", schemaVersion)); err != nil {
		return err
	}
	return tx.Commit()
}
