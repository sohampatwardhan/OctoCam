use rusqlite::{params, Connection, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct User {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub role: String,
    pub created_at: String,
}

impl User {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Passkey {
    pub id: i64,
    pub user_id: i64,
    pub credential_id: Vec<u8>,
    pub public_key: Vec<u8>,
    pub counter: u32,
    pub name: String,
    pub transports: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn init(db_path: &Path) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let conn = Connection::open(db_path)?;
        
        conn.execute_batch(
            "PRAGMA journal_mode = WAL;
             PRAGMA foreign_keys = ON;"
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS users (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                role TEXT NOT NULL DEFAULT 'admin',
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                updated_at DATETIME DEFAULT CURRENT_TIMESTAMP
            );",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS passkeys (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                user_id INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
                credential_id BLOB UNIQUE NOT NULL,
                public_key BLOB NOT NULL,
                counter INTEGER NOT NULL DEFAULT 0,
                name TEXT NOT NULL,
                transports TEXT,
                created_at DATETIME DEFAULT CURRENT_TIMESTAMP,
                last_used_at DATETIME
            );",
            [],
        )?;

        conn.execute(
            "CREATE TABLE IF NOT EXISTS webauthn_challenges (
                id TEXT PRIMARY KEY,
                challenge BLOB NOT NULL,
                user_id INTEGER,
                purpose TEXT NOT NULL,
                expires_at INTEGER NOT NULL
            );",
            [],
        )?;

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    pub fn migrate_legacy_password(&self, password_hash: &str) -> Result<()> {
        if password_hash.trim().is_empty() {
            return Ok(());
        }
        if !self.has_users()? {
            let _ = self.create_user("admin", password_hash, "admin");
        }
        Ok(())
    }

    pub fn has_users(&self) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let count: i64 = conn.query_row("SELECT COUNT(*) FROM users", [], |row| row.get(0))?;
        Ok(count > 0)
    }

    pub fn create_user(&self, username: &str, password_hash: &str, role: &str) -> Result<User> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO users (username, password_hash, role) VALUES (?1, ?2, ?3)",
            params![username, password_hash, role],
        )?;
        let id = conn.last_insert_rowid();
        conn.query_row(
            "SELECT id, username, password_hash, role, created_at FROM users WHERE id = ?1",
            params![id],
            |row| {
                Ok(User {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    role: row.get(3)?,
                    created_at: row.get(4)?,
                })
            },
        )
    }

    pub fn get_user_by_username(&self, username: &str) -> Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, password_hash, role, created_at FROM users WHERE username = ?1",
        )?;
        let mut rows = stmt.query(params![username])?;
        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                role: row.get(3)?,
                created_at: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn get_user_by_id(&self, id: i64) -> Result<Option<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, password_hash, role, created_at FROM users WHERE id = ?1",
        )?;
        let mut rows = stmt.query(params![id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(User {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                role: row.get(3)?,
                created_at: row.get(4)?,
            }))
        } else {
            Ok(None)
        }
    }

    #[allow(dead_code)]
    pub fn list_users(&self) -> Result<Vec<User>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, username, password_hash, role, created_at FROM users ORDER BY id ASC",
        )?;
        let user_iter = stmt.query_map([], |row| {
            Ok(User {
                id: row.get(0)?,
                username: row.get(1)?,
                password_hash: row.get(2)?,
                role: row.get(3)?,
                created_at: row.get(4)?,
            })
        })?;
        let mut users = Vec::new();
        for user in user_iter {
            users.push(user?);
        }
        Ok(users)
    }

    pub fn update_password(&self, user_id: i64, new_hash: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE users SET password_hash = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![new_hash, user_id],
        )?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn delete_user(&self, user_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM users WHERE id = ?1", params![user_id])?;
        Ok(())
    }

    // Passkeys
    pub fn add_passkey(
        &self,
        user_id: i64,
        credential_id: &[u8],
        public_key: &[u8],
        name: &str,
        transports: Option<&str>,
    ) -> Result<Passkey> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO passkeys (user_id, credential_id, public_key, counter, name, transports)
             VALUES (?1, ?2, ?3, 0, ?4, ?5)",
            params![user_id, credential_id, public_key, name, transports],
        )?;
        let id = conn.last_insert_rowid();
        conn.query_row(
            "SELECT id, user_id, credential_id, public_key, counter, name, transports, created_at, last_used_at
             FROM passkeys WHERE id = ?1",
            params![id],
            |row| {
                Ok(Passkey {
                    id: row.get(0)?,
                    user_id: row.get(1)?,
                    credential_id: row.get(2)?,
                    public_key: row.get(3)?,
                    counter: row.get(4)?,
                    name: row.get(5)?,
                    transports: row.get(6)?,
                    created_at: row.get(7)?,
                    last_used_at: row.get(8)?,
                })
            },
        )
    }

    pub fn get_passkey_by_credential_id(&self, credential_id: &[u8]) -> Result<Option<Passkey>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, credential_id, public_key, counter, name, transports, created_at, last_used_at
             FROM passkeys WHERE credential_id = ?1",
        )?;
        let mut rows = stmt.query(params![credential_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some(Passkey {
                id: row.get(0)?,
                user_id: row.get(1)?,
                credential_id: row.get(2)?,
                public_key: row.get(3)?,
                counter: row.get(4)?,
                name: row.get(5)?,
                transports: row.get(6)?,
                created_at: row.get(7)?,
                last_used_at: row.get(8)?,
            }))
        } else {
            Ok(None)
        }
    }

    pub fn list_passkeys_for_user(&self, user_id: i64) -> Result<Vec<Passkey>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, credential_id, public_key, counter, name, transports, created_at, last_used_at
             FROM passkeys WHERE user_id = ?1 ORDER BY id DESC",
        )?;
        let pk_iter = stmt.query_map(params![user_id], |row| {
            Ok(Passkey {
                id: row.get(0)?,
                user_id: row.get(1)?,
                credential_id: row.get(2)?,
                public_key: row.get(3)?,
                counter: row.get(4)?,
                name: row.get(5)?,
                transports: row.get(6)?,
                created_at: row.get(7)?,
                last_used_at: row.get(8)?,
            })
        })?;
        let mut passkeys = Vec::new();
        for pk in pk_iter {
            passkeys.push(pk?);
        }
        Ok(passkeys)
    }

    pub fn list_all_passkeys(&self) -> Result<Vec<Passkey>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, user_id, credential_id, public_key, counter, name, transports, created_at, last_used_at
             FROM passkeys ORDER BY id DESC",
        )?;
        let pk_iter = stmt.query_map([], |row| {
            Ok(Passkey {
                id: row.get(0)?,
                user_id: row.get(1)?,
                credential_id: row.get(2)?,
                public_key: row.get(3)?,
                counter: row.get(4)?,
                name: row.get(5)?,
                transports: row.get(6)?,
                created_at: row.get(7)?,
                last_used_at: row.get(8)?,
            })
        })?;
        let mut passkeys = Vec::new();
        for pk in pk_iter {
            passkeys.push(pk?);
        }
        Ok(passkeys)
    }

    pub fn update_passkey_counter(&self, id: i64, new_counter: u32) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE passkeys SET counter = ?1, last_used_at = CURRENT_TIMESTAMP WHERE id = ?2",
            params![new_counter, id],
        )?;
        Ok(())
    }

    pub fn delete_passkey(&self, id: i64, user_id: i64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM passkeys WHERE id = ?1 AND user_id = ?2",
            params![id, user_id],
        )?;
        Ok(())
    }

    pub fn update_passkey_name(&self, id: i64, user_id: i64, name: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE passkeys SET name = ?1 WHERE id = ?2 AND user_id = ?3",
            params![name, id, user_id],
        )?;
        Ok(())
    }

    // Challenges
    pub fn save_challenge(
        &self,
        challenge_id: &str,
        challenge: &[u8],
        user_id: Option<i64>,
        purpose: &str,
        expires_at: i64,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO webauthn_challenges (id, challenge, user_id, purpose, expires_at)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![challenge_id, challenge, user_id, purpose, expires_at],
        )?;
        Ok(())
    }

    pub fn get_challenge(
        &self,
        challenge_id: &str,
    ) -> Result<Option<(Vec<u8>, Option<i64>, String)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT challenge, user_id, purpose FROM webauthn_challenges WHERE id = ?1 AND expires_at > strftime('%s', 'now')",
        )?;
        let mut rows = stmt.query(params![challenge_id])?;
        if let Some(row) = rows.next()? {
            Ok(Some((row.get(0)?, row.get(1)?, row.get(2)?)))
        } else {
            Ok(None)
        }
    }

    pub fn delete_challenge(&self, challenge_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM webauthn_challenges WHERE id = ?1",
            params![challenge_id],
        )?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn creates_and_manages_users() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::init(file.path()).unwrap();

        assert!(!db.has_users().unwrap());
        let user = db.create_user("octo_admin", "hash123", "admin").unwrap();
        assert_eq!(user.username, "octo_admin");
        assert!(db.has_users().unwrap());

        let fetched = db.get_user_by_username("octo_admin").unwrap().unwrap();
        assert_eq!(fetched.id, user.id);
        assert_eq!(fetched.password_hash, "hash123");

        db.update_password(user.id, "new_hash").unwrap();
        let updated = db.get_user_by_id(user.id).unwrap().unwrap();
        assert_eq!(updated.password_hash, "new_hash");
    }

    #[test]
    fn legacy_password_migration_imports_admin() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::init(file.path()).unwrap();

        db.migrate_legacy_password("pbkdf2_hash_old").unwrap();
        assert!(db.has_users().unwrap());

        let user = db.get_user_by_username("admin").unwrap().unwrap();
        assert_eq!(user.password_hash, "pbkdf2_hash_old");
    }

    #[test]
    fn manages_passkeys_and_challenges() {
        let file = NamedTempFile::new().unwrap();
        let db = Database::init(file.path()).unwrap();

        let user = db.create_user("admin", "hash", "admin").unwrap();
        let pk = db
            .add_passkey(user.id, b"cred1", b"pubkey1", "My Phone", Some("usb,nfc"))
            .unwrap();

        assert_eq!(pk.name, "My Phone");
        let fetched_pk = db
            .get_passkey_by_credential_id(b"cred1")
            .unwrap()
            .unwrap();
        assert_eq!(fetched_pk.id, pk.id);

        let user_pks = db.list_passkeys_for_user(user.id).unwrap();
        assert_eq!(user_pks.len(), 1);

        db.delete_passkey(pk.id, user.id).unwrap();
        assert!(db.list_passkeys_for_user(user.id).unwrap().is_empty());
    }
}
