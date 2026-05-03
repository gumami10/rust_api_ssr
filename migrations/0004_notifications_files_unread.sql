-- Add kind column to distinguish user messages from system notifications
ALTER TABLE chat_messages ADD COLUMN kind TEXT NOT NULL DEFAULT 'user';

-- Add file attachment columns
ALTER TABLE chat_messages ADD COLUMN file_name TEXT;
ALTER TABLE chat_messages ADD COLUMN file_data BLOB;
ALTER TABLE chat_messages ADD COLUMN file_content_type TEXT;

-- Track per-user read position per room for unread badges
CREATE TABLE IF NOT EXISTS chat_room_read_positions (
    room_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    last_read_message_id INTEGER NOT NULL DEFAULT 0,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (room_id, user_id),
    FOREIGN KEY (room_id) REFERENCES chat_rooms(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE
);
