CREATE TABLE entry_receive_attempt (
    entry_id           TEXT   NOT NULL PRIMARY KEY,
    current_attempt_id TEXT   NOT NULL,
    attempt_state      TEXT   NOT NULL,
    updated_at_ms      BIGINT NOT NULL
);
