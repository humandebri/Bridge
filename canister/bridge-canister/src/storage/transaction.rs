use super::{params, DbError, PreparedAuditBatch, SqlCodec, UpdateConnection};

pub(super) fn increment_table_count(
    connection: &UpdateConnection<'_>,
    table: &str,
) -> Result<(), DbError> {
    let bytes = connection.query_scalar::<Vec<u8>>(
        "SELECT count FROM table_counts WHERE name = ?1",
        params![table],
    )?;
    let count = u64::from_sql_bytes(bytes).map_err(|_| DbError::TypeMismatch {
        index: 0,
        expected: "u64 big-endian blob",
        actual: "invalid blob",
    })?;
    let next = count
        .checked_add(1)
        .ok_or_else(|| DbError::Constraint("table count overflow".into()))?;
    connection.execute(
        "UPDATE table_counts SET count = ?1 WHERE name = ?2",
        params![next.to_sql_bytes(), table],
    )
}

pub(super) fn read_table_count(
    connection: &UpdateConnection<'_>,
    table: &str,
) -> Result<u64, DbError> {
    let raw = connection.query_scalar::<Vec<u8>>(
        "SELECT count FROM table_counts WHERE name = ?1",
        params![table],
    )?;
    u64::from_sql_bytes(raw).map_err(|_| DbError::Constraint("invalid table count".into()))
}

pub(super) fn decrement_table_count(
    connection: &UpdateConnection<'_>,
    table: &str,
) -> Result<(), DbError> {
    let bytes = connection.query_scalar::<Vec<u8>>(
        "SELECT count FROM table_counts WHERE name = ?1",
        params![table],
    )?;
    let count = u64::from_sql_bytes(bytes).map_err(|_| DbError::TypeMismatch {
        index: 0,
        expected: "u64 big-endian blob",
        actual: "invalid blob",
    })?;
    let next = count
        .checked_sub(1)
        .ok_or_else(|| DbError::Constraint("table count underflow".into()))?;
    connection.execute(
        "UPDATE table_counts SET count = ?1 WHERE name = ?2",
        params![next.to_sql_bytes(), table],
    )
}

pub(super) fn expect_blob(
    connection: &UpdateConnection<'_>,
    sql: &'static str,
    values: &[&dyn ic_sqlite_vfs::db::ToSql],
    expected: &[u8],
    stale_error: &'static str,
) -> Result<(), DbError> {
    let persisted = connection.query_scalar::<Vec<u8>>(sql, values)?;
    if persisted != expected {
        return Err(DbError::Constraint(stale_error.into()));
    }
    Ok(())
}

pub(super) fn expect_optional_blob(
    connection: &UpdateConnection<'_>,
    sql: &'static str,
    values: &[&dyn ic_sqlite_vfs::db::ToSql],
    expected: Option<&[u8]>,
    stale_error: &'static str,
) -> Result<(), DbError> {
    let persisted = connection.query_optional_scalar::<Vec<u8>>(sql, values)?;
    if persisted.as_deref() != expected {
        return Err(DbError::Constraint(stale_error.into()));
    }
    Ok(())
}

pub(super) fn insert_tracked_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    key: Vec<u8>,
    value: Vec<u8>,
) -> Result<(), DbError> {
    let sql = format!("INSERT INTO {table}(key, value) VALUES (?1, ?2)");
    connection.execute(&sql, params![key, value])?;
    increment_table_count(connection, table)
}

fn require_tracked_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    key: &[u8],
) -> Result<(), DbError> {
    let sql = format!("SELECT 1 FROM {table} WHERE key = ?1");
    if connection
        .query_optional_scalar::<i64>(&sql, params![key])?
        .is_none()
    {
        return Err(DbError::Constraint(format!(
            "missing tracked entry in {table}"
        )));
    }
    Ok(())
}

pub(super) fn delete_tracked_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    key: Vec<u8>,
) -> Result<(), DbError> {
    require_tracked_entry(connection, table, &key)?;
    let sql = format!("DELETE FROM {table} WHERE key = ?1");
    connection.execute(&sql, params![key])?;
    decrement_table_count(connection, table)
}

pub(super) fn transition_tracked_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    previous_key: Option<Vec<u8>>,
    next: Option<(Vec<u8>, Vec<u8>)>,
) -> Result<(), DbError> {
    match (previous_key, next) {
        (None, None) => Ok(()),
        (Some(previous), None) => delete_tracked_entry(connection, table, previous),
        (None, Some((key, value))) => insert_tracked_entry(connection, table, key, value),
        (Some(previous), Some((next, value))) if previous == next => {
            require_tracked_entry(connection, table, &next)?;
            let sql = format!("UPDATE {table} SET value = ?1 WHERE key = ?2");
            connection.execute(&sql, params![value, next])
        }
        (Some(previous), Some((next, value))) => {
            delete_tracked_entry(connection, table, previous)?;
            insert_tracked_entry(connection, table, next, value)
        }
    }
}

pub(super) fn replace_expected_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    key: Vec<u8>,
    expected: &[u8],
    next: Vec<u8>,
    stale_error: &'static str,
) -> Result<(), DbError> {
    let select_sql = format!("SELECT value FROM {table} WHERE key = ?1");
    let persisted = connection.query_scalar::<Vec<u8>>(&select_sql, params![key.clone()])?;
    if persisted != expected {
        return Err(DbError::Constraint(stale_error.into()));
    }
    let update_sql = format!("UPDATE {table} SET value = ?1 WHERE key = ?2");
    connection.execute(&update_sql, params![next, key])
}

pub(super) fn commit_audit_batch(
    connection: &UpdateConnection<'_>,
    audit: &PreparedAuditBatch,
) -> Result<(), DbError> {
    for (sequence, event_blob) in &audit.events {
        insert_tracked_entry(
            connection,
            "audit_events",
            sequence.to_sql_bytes(),
            event_blob.to_sql_bytes(),
        )?;
    }
    for sequence in &audit.pruned_sequences {
        delete_tracked_entry(connection, "audit_events", sequence.to_sql_bytes())?;
    }
    Ok(())
}

pub(super) fn upsert_table_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    key: Vec<u8>,
    value: Vec<u8>,
) -> Result<(), DbError> {
    let select_sql = format!("SELECT 1 FROM {table} WHERE key = ?1");
    let update_sql = format!("UPDATE {table} SET value = ?1 WHERE key = ?2");
    if connection
        .query_optional_scalar::<i64>(&select_sql, params![key.clone()])?
        .is_some()
    {
        connection.execute(&update_sql, params![value, key])?;
    } else {
        insert_tracked_entry(connection, table, key, value)?;
    }
    Ok(())
}

pub(super) fn remove_table_entry(
    connection: &UpdateConnection<'_>,
    table: &'static str,
    key: Vec<u8>,
) -> Result<(), DbError> {
    let select_sql = format!("SELECT 1 FROM {table} WHERE key = ?1");
    if connection
        .query_optional_scalar::<i64>(&select_sql, params![key.clone()])?
        .is_some()
    {
        delete_tracked_entry(connection, table, key)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DefaultMemoryImpl, StableStore};

    #[test]
    fn cas_helpers_reject_stale_required_and_optional_blobs_without_writes() {
        let store = StableStore::init(DefaultMemoryImpl::default()).expect("initialize");
        let counters = store.counters.get().expect("counters");
        store
            .handle
            .update(|connection| {
                expect_blob(
                    connection,
                    "SELECT counters FROM singleton_state WHERE id = 1",
                    params![],
                    counters.as_slice(),
                    "stale counters",
                )?;
                expect_optional_blob(
                    connection,
                    "SELECT value FROM open_hold_index WHERE key = ?1",
                    params![1u64.to_sql_bytes()],
                    None,
                    "stale optional row",
                )
            })
            .expect("matching CAS");

        let failed = store.handle.update(|connection| {
            expect_blob(
                connection,
                "SELECT counters FROM singleton_state WHERE id = 1",
                params![],
                &[0xff],
                "stale counters",
            )?;
            insert_tracked_entry(
                connection,
                "open_hold_index",
                1u64.to_sql_bytes(),
                0u8.to_sql_bytes(),
            )
        });
        assert!(matches!(failed, Err(DbError::Constraint(message)) if message == "stale counters"));
        assert_eq!(store.table_count("open_hold_index"), 0);
    }

    #[test]
    fn tracked_transition_covers_all_membership_changes_and_keeps_counts_exact() {
        let store = StableStore::init(DefaultMemoryImpl::default()).expect("initialize");
        let first = 1u64.to_sql_bytes();
        let second = 2u64.to_sql_bytes();
        store
            .handle
            .update(|connection| {
                transition_tracked_entry(connection, "open_hold_index", None, None)?;
                transition_tracked_entry(
                    connection,
                    "open_hold_index",
                    None,
                    Some((first.clone(), 0u8.to_sql_bytes())),
                )?;
                transition_tracked_entry(
                    connection,
                    "open_hold_index",
                    Some(first.clone()),
                    Some((first.clone(), 1u8.to_sql_bytes())),
                )?;
                transition_tracked_entry(
                    connection,
                    "open_hold_index",
                    Some(first),
                    Some((second.clone(), 0u8.to_sql_bytes())),
                )?;
                assert_eq!(read_table_count(connection, "open_hold_index")?, 1);
                transition_tracked_entry(connection, "open_hold_index", Some(second), None)?;
                assert_eq!(read_table_count(connection, "open_hold_index")?, 0);
                Ok(())
            })
            .expect("transitions");
    }

    #[test]
    fn tracked_transitions_roll_back_when_expected_membership_is_missing_or_conflicting() {
        let store = StableStore::init(DefaultMemoryImpl::default()).expect("initialize");
        let first = 1u64.to_sql_bytes();
        let missing = 2u64.to_sql_bytes();
        let conflicting = 3u64.to_sql_bytes();
        store
            .handle
            .update(|connection| {
                insert_tracked_entry(
                    connection,
                    "open_hold_index",
                    first.clone(),
                    0u8.to_sql_bytes(),
                )?;
                insert_tracked_entry(
                    connection,
                    "open_hold_index",
                    conflicting.clone(),
                    0u8.to_sql_bytes(),
                )
            })
            .expect("seed tracked entries");

        assert!(store
            .handle
            .update(|connection| delete_tracked_entry(
                connection,
                "open_hold_index",
                missing.clone()
            ))
            .is_err());
        assert!(store
            .handle
            .update(|connection| transition_tracked_entry(
                connection,
                "open_hold_index",
                Some(missing.clone()),
                Some((missing.clone(), 1u8.to_sql_bytes())),
            ))
            .is_err());
        assert!(store
            .handle
            .update(|connection| transition_tracked_entry(
                connection,
                "open_hold_index",
                Some(missing.clone()),
                Some((4u64.to_sql_bytes(), 0u8.to_sql_bytes())),
            ))
            .is_err());
        assert!(store
            .handle
            .update(|connection| transition_tracked_entry(
                connection,
                "open_hold_index",
                Some(first.clone()),
                Some((conflicting.clone(), 0u8.to_sql_bytes())),
            ))
            .is_err());

        assert_eq!(store.table_count("open_hold_index"), 2);
        assert_eq!(store.open_hold_index.len(), 2);
        assert_eq!(store.open_hold_index.get(&1), Some(0));
        assert_eq!(store.open_hold_index.get(&3), Some(0));
        assert_eq!(store.open_hold_index.get(&2), None);
        assert_eq!(store.open_hold_index.get(&4), None);
    }
}
