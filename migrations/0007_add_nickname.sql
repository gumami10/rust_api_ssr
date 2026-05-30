ALTER TABLE users ADD COLUMN nickname TEXT;
CREATE UNIQUE INDEX idx_users_nickname ON users(nickname) WHERE nickname IS NOT NULL;
