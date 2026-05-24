-- End-to-end encryption support

-- Track whether a message is encrypted (server only stores ciphertext for private rooms)
ALTER TABLE chat_messages ADD COLUMN is_encrypted INTEGER NOT NULL DEFAULT 0;

-- Store user public keys for wrapping room keys
CREATE TABLE user_public_keys (
    user_id INTEGER PRIMARY KEY,
    public_key TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id)
);

-- Store room keys encrypted for each member (server never sees plaintext room key)
CREATE TABLE chat_room_keys (
    room_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    encrypted_key TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (room_id, user_id),
    FOREIGN KEY (room_id) REFERENCES chat_rooms(id),
    FOREIGN KEY (user_id) REFERENCES users(id)
);
