-- Multi-device end-to-end encryption support.

CREATE TABLE user_devices (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    device_name TEXT,
    public_key TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    last_seen_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    UNIQUE(user_id, device_id)
);

INSERT INTO user_devices (user_id, device_id, device_name, public_key, created_at, last_seen_at)
SELECT
    user_id,
    'legacy-' || user_id,
    'Legacy device',
    public_key,
    created_at,
    created_at
FROM user_public_keys;

CREATE TABLE chat_room_device_keys (
    room_id INTEGER NOT NULL,
    user_id INTEGER NOT NULL,
    device_id TEXT NOT NULL,
    encrypted_key TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (room_id, user_id, device_id),
    FOREIGN KEY (room_id) REFERENCES chat_rooms(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id) REFERENCES users(id) ON DELETE CASCADE,
    FOREIGN KEY (user_id, device_id) REFERENCES user_devices(user_id, device_id) ON DELETE CASCADE
);

INSERT INTO chat_room_device_keys (room_id, user_id, device_id, encrypted_key, created_at)
SELECT
    room_id,
    user_id,
    'legacy-' || user_id,
    encrypted_key,
    created_at
FROM chat_room_keys
WHERE EXISTS (
    SELECT 1
    FROM user_devices
    WHERE user_devices.user_id = chat_room_keys.user_id
      AND user_devices.device_id = 'legacy-' || chat_room_keys.user_id
);

DROP TABLE chat_room_keys;
DROP TABLE user_public_keys;
