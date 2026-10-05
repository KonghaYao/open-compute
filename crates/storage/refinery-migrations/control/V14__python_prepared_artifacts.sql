-- Dedicated snapshots are immutable Version artifacts, never a request-time cache.
CREATE TABLE version_python_prepared (
  version_id TEXT PRIMARY KEY REFERENCES worker_versions(id),
  prepared_identity_sha256 BLOB NOT NULL CHECK(length(prepared_identity_sha256) = 32),
  identity_json BLOB NOT NULL CHECK(length(identity_json) BETWEEN 1 AND 65536),
  artifact_sha256 BLOB NOT NULL CHECK(length(artifact_sha256) = 32),
  -- 128 MiB native snapshot + bounded envelope header/framing + AEAD tag.
  artifact_size INTEGER NOT NULL CHECK(artifact_size BETWEEN 1 AND 134283292),
  created_at_ms INTEGER NOT NULL,
  CHECK(json_valid(CAST(identity_json AS TEXT))),
  CHECK(json_extract(CAST(identity_json AS TEXT), '$.schemaVersion') IS 1),
  CHECK(json_extract(CAST(identity_json AS TEXT), '$.versionId') IS version_id)
) WITHOUT ROWID, STRICT;

CREATE TRIGGER version_python_prepared_insert_guard
BEFORE INSERT ON version_python_prepared
BEGIN
  SELECT CASE WHEN NOT EXISTS (
    SELECT 1 FROM worker_versions v
    JOIN workers w ON w.id = v.worker_id
    WHERE v.id = NEW.version_id AND v.state = 'validating'
      AND v.content_kind = 'worker' AND substr(v.main_module, -3) = '.py'
      AND w.deleted_at_ms IS NULL
      AND json_extract(CAST(NEW.identity_json AS TEXT), '$.instanceId')
          IS (SELECT instance_id FROM instance_identity)
      AND json_extract(CAST(NEW.identity_json AS TEXT), '$.workerId') IS w.id
      AND json_extract(CAST(NEW.identity_json AS TEXT), '$.workerCodeSha256')
          IS lower(hex(v.worker_code_sha256))
  ) THEN RAISE(ABORT, 'Python prepared artifact authority invariant') END;
END;

CREATE TRIGGER version_python_prepared_update_guard
BEFORE UPDATE ON version_python_prepared
BEGIN
  SELECT RAISE(ABORT, 'immutable Python prepared artifact');
END;

CREATE TRIGGER version_python_prepared_delete_guard
BEFORE DELETE ON version_python_prepared
WHEN (SELECT state FROM worker_versions WHERE id = OLD.version_id) != 'deleting'
BEGIN
  SELECT RAISE(ABORT, 'immutable Python prepared artifact');
END;

CREATE TRIGGER version_python_ready_guard
BEFORE UPDATE OF state ON worker_versions
WHEN NEW.state = 'ready' AND NEW.content_kind = 'worker'
  AND substr(NEW.main_module, -3) = '.py'
  AND NOT EXISTS (SELECT 1 FROM version_python_prepared WHERE version_id = NEW.id)
BEGIN
  SELECT RAISE(ABORT, 'Python prepared artifact is required before readiness');
END;
