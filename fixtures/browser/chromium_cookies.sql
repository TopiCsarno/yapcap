PRAGMA user_version = 24;

CREATE TABLE cookies(
  creation_utc INTEGER NOT NULL,
  host_key TEXT NOT NULL,
  top_frame_site_key TEXT NOT NULL,
  name TEXT NOT NULL,
  value TEXT NOT NULL,
  encrypted_value BLOB NOT NULL,
  path TEXT NOT NULL,
  expires_utc INTEGER NOT NULL,
  is_secure INTEGER NOT NULL,
  is_httponly INTEGER NOT NULL,
  last_access_utc INTEGER NOT NULL,
  has_expires INTEGER NOT NULL,
  is_persistent INTEGER NOT NULL,
  priority INTEGER NOT NULL,
  samesite INTEGER NOT NULL,
  source_scheme INTEGER NOT NULL,
  source_port INTEGER NOT NULL,
  last_update_utc INTEGER NOT NULL,
  source_type INTEGER NOT NULL,
  has_cross_site_ancestor INTEGER NOT NULL
);

INSERT INTO cookies VALUES
  (13300000000000000, '.cursor.com', '', 'WorkosCursorSessionToken', 'cursor-test-session-token', X'', '/', 13400000000000000, 1, 1, 13300000000000000, 1, 1, 1, 0, 2, 443, 13300000000000000, 0, 0),
  (13300000000000001, 'cursor.com', '', 'other_cookie', 'ignore-me', X'', '/', 13400000000000000, 1, 0, 13300000000000001, 1, 1, 1, 0, 2, 443, 13300000000000001, 0, 0),
  (13300000000000002, '.example.com', '', 'WorkosCursorSessionToken', 'wrong-host-token', X'', '/', 13400000000000000, 1, 1, 13300000000000002, 1, 1, 1, 0, 2, 443, 13300000000000002, 0, 0);
