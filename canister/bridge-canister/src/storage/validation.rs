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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn row_shape_validation_rejects_either_dimension() {
        assert!(expect_row_shape(&[0; 8], &[0], 8, 1, "shape").is_ok());
        assert!(matches!(
            expect_row_shape(&[0; 7], &[0], 8, 1, "shape"),
            Err(DbError::Constraint(message)) if message == "shape"
        ));
        assert!(matches!(
            expect_row_shape(&[0; 8], &[], 8, 1, "shape"),
            Err(DbError::Constraint(message)) if message == "shape"
        ));
    }
}
