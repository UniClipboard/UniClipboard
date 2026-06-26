-- Add a user-state favorite flag to clipboard_entry.
--
-- is_favorited = 0 means "not favorited" (the default for all existing and
-- newly captured entries). The flag is toggled by an explicit user action and
-- is independent of representation selection; pinned (quota exemption) is a
-- separate concept and is left untouched.

ALTER TABLE clipboard_entry
ADD COLUMN is_favorited INTEGER NOT NULL DEFAULT 0;
