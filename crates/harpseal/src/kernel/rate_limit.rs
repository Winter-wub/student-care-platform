//! Fixed-window counters in the `rate_limit` table (migration 0007), for
//! limits that must hold across Worker isolates. A caller picks a key
//! (`<area>:<subject>`) and a window number (e.g. the epoch minute), calls
//! [`hit`] once per attempt and compares the count it returns with its own
//! threshold. Rows are tiny; [`prune`] deletes old windows.

use crate::port_types::{Statement, Value};
use crate::{Result, Services};

/// Counts one attempt for `key` in `window` and returns the count so far,
/// this one included. One `UPSERT ... RETURNING`, so racing attempts each
/// get their own number.
pub(crate) async fn hit(services: &Services, key: &str, window: i64) -> Result<i64> {
    let stmt = Statement::new(
        "INSERT INTO rate_limit (key, window, n) VALUES (?, ?, 1) \
         ON CONFLICT (key, window) DO UPDATE SET n = n + 1 RETURNING n",
    )
    .bind(key)
    .bind(window);
    let rows = services.db.query(stmt).await?;
    Ok(rows.get(0, "n").and_then(Value::as_i64).unwrap_or(1))
}

/// Deletes at most `limit` rows of keys starting with `prefix` whose
/// window is below `before` (windows are per key family, so each family
/// prunes its own); returns how many.
pub(crate) async fn prune(
    services: &Services,
    prefix: &str,
    before: i64,
    limit: i64,
) -> Result<u64> {
    let stmt = Statement::new(
        "DELETE FROM rate_limit WHERE rowid IN (SELECT rowid FROM rate_limit \
         WHERE substr(key, 1, length(?)) = ? AND window < ? LIMIT ?)",
    )
    .bind(prefix)
    .bind(prefix)
    .bind(before)
    .bind(limit);
    Ok(services.db.execute(stmt).await?.rows_affected)
}
