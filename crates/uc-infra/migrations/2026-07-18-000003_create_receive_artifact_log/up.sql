CREATE TABLE receive_artifact_log (
    entry_id            TEXT NOT NULL,
    attempt_id          TEXT NOT NULL,
    phase               TEXT NOT NULL,
    resolution          TEXT NOT NULL,
    artifact_ciphertext BLOB NOT NULL,
    updated_at_ms       BIGINT NOT NULL,
    PRIMARY KEY (entry_id, attempt_id)
);

CREATE INDEX idx_receive_artifact_resolution
    ON receive_artifact_log(resolution, updated_at_ms);
