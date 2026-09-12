use super::DbError;

pub(super) fn expect_row_shape(
    key: &[u8],
    value: &[u8],
    key_len: usize,
    value_len: usize,
    error: &'static str,
) -> Result<(), DbError> {
    if key.len() != key_len || value.len() != value_len {
        return Err(DbError::Constraint(error.into()));
    }
    Ok(())
}
