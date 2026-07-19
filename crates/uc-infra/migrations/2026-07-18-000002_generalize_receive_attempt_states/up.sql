UPDATE entry_receive_attempt SET attempt_state = 'receiving' WHERE attempt_state = 'active';
UPDATE entry_receive_attempt SET attempt_state = 'committing' WHERE attempt_state = 'promoting';
UPDATE entry_receive_attempt SET attempt_state = 'completed' WHERE attempt_state = 'published';
