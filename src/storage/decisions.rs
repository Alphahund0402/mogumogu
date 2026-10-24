//! Local "keep" decisions for package versions; nothing is ever changed.
use super::Repository;
use crate::Result;
use rusqlite::params;

impl Repository {
    pub fn retained_packages(&mut self) -> Result<Vec<String>> {
        self.query(
            "SELECT package_key FROM package_decisions WHERE retained=1 ORDER BY package_key LIMIT 10000",
            [],
            |r| r.get(0),
        )
    }

    pub fn retain_package(&mut self, key: &str, retained: bool) -> Result<()> {
        self.require_local()?;
        if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(crate::Error::invalid("Paket-Schlüssel aus dem Paketbericht verwenden."));
        }
        let key = key.to_ascii_lowercase();
        let now = self.now();
        self.write(|tx| {
            if retained {
                let count: i64 = tx.query_row("SELECT COUNT(*) FROM package_decisions", [], |r| r.get(0))?;
                if count >= 10000 {
                    return Err(crate::Error::blocked("Limit für Behalten-Entscheidungen erreicht."));
                }
                tx.execute(
                    "INSERT INTO package_decisions(package_key,retained,updated_at) VALUES(?1,1,?2)
                    ON CONFLICT(package_key) DO UPDATE SET retained=1,updated_at=excluded.updated_at",
                    params![key, now],
                )?;
            } else {
                tx.execute("DELETE FROM package_decisions WHERE package_key=?1", [key])?;
            }
            Ok(())
        })
    }
}
