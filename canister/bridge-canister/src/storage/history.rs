use super::*;

pub(super) fn withdrawal_history_key(record: &WithdrawalRecord) -> Vec<u8> {
    let mut key = record.base_requester.to_vec();
    key.extend_from_slice(&(u64::MAX - record.observed_at_ns).to_be_bytes());
    key.extend_from_slice(&record.id.bytes());
    key
}

pub(super) fn write_withdrawal_history(
    connection: &UpdateConnection<'_>,
    record: &WithdrawalRecord,
) -> Result<(), DbError> {
    upsert_table_entry(
        connection,
        "withdrawal_requester_index",
        withdrawal_history_key(record),
        record.id.bytes().to_vec(),
    )
}

pub(super) fn write_withdrawal_transaction(
    connection: &UpdateConnection<'_>,
    id: &[u8],
    hash: &[u8; 32],
) -> Result<(), DbError> {
    let previous = connection.query_optional_scalar::<Vec<u8>>(
        "SELECT value FROM withdrawal_transaction_index WHERE key = ?1",
        params![id],
    )?;
    if previous
        .as_ref()
        .is_some_and(|value| value.as_slice() != hash)
    {
        return Err(DbError::Constraint(
            "conflicting withdrawal transaction".into(),
        ));
    }
    upsert_table_entry(
        connection,
        "withdrawal_transaction_index",
        id.to_vec(),
        hash.to_vec(),
    )
}

pub(super) fn create_history_indexes(connection: &UpdateConnection<'_>) -> Result<(), DbError> {
    for table in ["withdrawal_requester_index", "withdrawal_transaction_index"] {
        connection.execute(&format!("CREATE TABLE {table} (key BLOB PRIMARY KEY NOT NULL, value BLOB NOT NULL) STRICT, WITHOUT ROWID"), params![])?;
        connection.execute(
            "INSERT INTO table_counts(name, count) VALUES (?1, ?2)",
            params![table, 0u64.to_sql_bytes()],
        )?;
    }
    connection.execute("CREATE TABLE history_index_progress (id INTEGER PRIMARY KEY CHECK(id=1), stage INTEGER NOT NULL, cursor BLOB NOT NULL) STRICT", params![])?;
    connection.execute(
        "INSERT INTO history_index_progress VALUES (1, 0, X'')",
        params![],
    )
}

impl StableStore {
    /// Each message commits at most 100 rows together with its restart cursor.
    pub fn advance_history_indexes(&mut self) -> Result<bool, StorageError> {
        self.handle
            .update(|connection| {
                let (stage, cursor) = connection.query_one(
                    "SELECT stage, cursor FROM history_index_progress WHERE id = 1",
                    params![],
                    |row| Ok((row.get::<i64>(0)?, row.get::<Vec<u8>>(1)?)),
                )?;
                if stage == 2 {
                    return Ok(true);
                }
                let table = match stage {
                    0 => "withdrawal_notification_index",
                    1 => "withdrawals",
                    _ => {
                        return Err(DbError::Constraint(
                            "invalid history migration stage".into(),
                        ))
                    }
                };
                let rows = connection.query_all(
                    &format!(
                        "SELECT key, value FROM {table} WHERE key > ?1 ORDER BY key LIMIT 100"
                    ),
                    params![cursor],
                    |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?)),
                )?;
                for (key, value) in &rows {
                    if stage == 0 {
                        let hash: [u8; 32] = key
                            .as_slice()
                            .try_into()
                            .map_err(|_| DbError::Constraint("invalid transaction hash".into()))?;
                        write_withdrawal_transaction(connection, value, &hash)?;
                    } else {
                        write_withdrawal_history(
                            connection,
                            &decode_withdrawal_blob(value.clone())?,
                        )?;
                    }
                }
                let (next_stage, next_cursor) = if rows.len() < 100 {
                    (stage + 1, Vec::new())
                } else {
                    (
                        stage,
                        rows.last().expect("nonempty bounded batch").0.clone(),
                    )
                };
                connection.execute(
                    "UPDATE history_index_progress SET stage = ?1, cursor = ?2 WHERE id = 1",
                    params![next_stage, next_cursor],
                )?;
                Ok(next_stage == 2)
            })
            .map_err(Into::into)
    }

    pub fn history_indexes_ready(&self) -> Result<bool, StorageError> {
        Ok(self.handle.query(|connection| {
            connection.query_scalar::<i64>(
                "SELECT stage FROM history_index_progress WHERE id = 1",
                params![],
            )
        })? == 2)
    }

    pub fn withdrawal_transaction_hash(
        &self,
        id: [u8; 32],
    ) -> Result<Option<Vec<u8>>, StorageError> {
        self.handle
            .query(|connection| {
                connection.query_optional_scalar::<Vec<u8>>(
                    "SELECT value FROM withdrawal_transaction_index WHERE key = ?1",
                    params![id.to_vec()],
                )
            })
            .map_err(Into::into)
    }

    pub fn withdrawal_history_page(
        &self,
        requester: [u8; 20],
        cursor: Option<&[u8]>,
        limit: u16,
    ) -> Result<Vec<(Vec<u8>, WithdrawalRecord)>, StorageError> {
        let mut after = requester.to_vec();
        if let Some(cursor) = cursor {
            after.extend_from_slice(cursor);
        }
        let mut end = requester.to_vec();
        end.extend_from_slice(&[0xff; 40]);
        let rows = self.handle.query(|connection| connection.query_all(
            "SELECT i.key, w.value FROM withdrawal_requester_index i JOIN withdrawals w ON w.key = i.value WHERE i.key > ?1 AND i.key <= ?2 ORDER BY i.key LIMIT ?3",
            params![after, end, i64::from(limit)], |row| Ok((row.get::<Vec<u8>>(0)?, row.get::<Vec<u8>>(1)?))))?;
        rows.into_iter()
            .map(|(key, raw)| Ok((key[20..].to_vec(), decode_withdrawal_blob(raw)?)))
            .collect()
    }
}
