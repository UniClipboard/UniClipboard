ALTER TABLE file_transfer ADD COLUMN attempt_id TEXT;
CREATE INDEX idx_file_transfer_entry_attempt ON file_transfer(entry_id, attempt_id);
