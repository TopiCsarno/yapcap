PRAGMA user_version = 17;

CREATE TABLE moz_cookies(
  id INTEGER PRIMARY KEY,
  originAttributes TEXT NOT NULL DEFAULT '',
  name TEXT,
  value TEXT,
  host TEXT,
  path TEXT,
  expiry INTEGER,
  lastAccessed INTEGER,
  creationTime INTEGER,
  isSecure INTEGER,
  isHttpOnly INTEGER,
  inBrowserElement INTEGER DEFAULT 0,
  sameSite INTEGER DEFAULT 0,
  schemeMap INTEGER DEFAULT 0,
  isPartitionedAttributeSet INTEGER DEFAULT 0,
  updateTime INTEGER,
  CONSTRAINT moz_uniqueid UNIQUE (name, host, path, originAttributes)
);

INSERT INTO moz_cookies VALUES
  (1, '', 'WorkosCursorSessionToken', 'cursor-test-session-token', '.cursor.com', '/', 1893456000, 1730000000000000, 1730000000000000, 1, 1, 0, 0, 2, 0, 1730000000000000),
  (2, '', 'other_cookie', 'ignore-me', 'cursor.com', '/', 1893456000, 1730000000000001, 1730000000000001, 1, 0, 0, 0, 2, 0, 1730000000000001),
  (3, '', 'WorkosCursorSessionToken', 'wrong-host-token', '.example.com', '/', 1893456000, 1730000000000002, 1730000000000002, 1, 1, 0, 0, 2, 0, 1730000000000002);
