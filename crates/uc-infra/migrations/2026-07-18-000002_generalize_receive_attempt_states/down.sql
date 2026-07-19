UPDATE entry_receive_attempt SET attempt_state = 'active' WHERE attempt_state = 'receiving';
UPDATE entry_receive_attempt SET attempt_state = 'promoting' WHERE attempt_state IN ('committing', 'cancelling', 'failing');
UPDATE entry_receive_attempt SET attempt_state = 'published' WHERE attempt_state = 'completed';
