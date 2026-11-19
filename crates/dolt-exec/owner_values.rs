//! Exact SQLite storage values used by Chronolog's owner policy.

use rusqlite::ffi as sys;
use std::ffi::{c_char, c_int};

#[derive(PartialEq, Eq)]
enum StoredValue<'a> {
    Null,
    Integer(i64),
    RealBits(u64),
    Text(&'a [u8]),
    Blob(&'a [u8]),
}

impl StoredValue<'_> {
    fn encoded_size(&self) -> usize {
        match self {
            Self::Null => 2,
            Self::Integer(_) | Self::RealBits(_) => 18,
            Self::Text(bytes) | Self::Blob(bytes) => {
                bytes.len().saturating_mul(2).saturating_add(10)
            }
        }
    }

    fn append(&self, key: &mut Vec<u8>) {
        match self {
            Self::Null => key.extend_from_slice(b"00"),
            Self::Integer(number) => {
                key.extend_from_slice(b"01");
                append_hex(key, &number.to_be_bytes());
            }
            Self::RealBits(bits) => {
                key.extend_from_slice(b"02");
                append_hex(key, &bits.to_be_bytes());
            }
            Self::Text(bytes) | Self::Blob(bytes) => {
                key.extend_from_slice(if matches!(self, Self::Text(_)) {
                    b"03"
                } else {
                    b"04"
                });
                append_hex(key, &(bytes.len() as u32).to_be_bytes());
                append_hex(key, bytes);
            }
        }
    }
}

fn append_hex(buffer: &mut Vec<u8>, bytes: &[u8]) {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    for byte in bytes {
        buffer.push(DIGITS[(byte >> 4) as usize]);
        buffer.push(DIGITS[(byte & 15) as usize]);
    }
}

/// The SQLite value is borrowed only until the callback returns. For TEXT or
/// BLOB the caller must not perform a converting read of the same value while
/// the returned bytes are borrowed.
unsafe fn stored_value<'a>(value: *mut sys::sqlite3_value) -> Option<StoredValue<'a>> {
    let kind = sys::sqlite3_value_type(value);
    let stored = match kind {
        sys::SQLITE_NULL => StoredValue::Null,
        sys::SQLITE_INTEGER => StoredValue::Integer(sys::sqlite3_value_int64(value)),
        sys::SQLITE_FLOAT => StoredValue::RealBits(sys::sqlite3_value_double(value).to_bits()),
        _ => {
            let data = if kind == sys::SQLITE_TEXT {
                sys::sqlite3_value_text(value)
            } else {
                sys::sqlite3_value_blob(value).cast::<u8>()
            };
            let length = sys::sqlite3_value_bytes(value) as usize;
            let bytes = if length == 0 {
                &[]
            } else if data.is_null() {
                return None;
            } else {
                std::slice::from_raw_parts(data, length)
            };
            if kind == sys::SQLITE_TEXT {
                StoredValue::Text(bytes)
            } else {
                StoredValue::Blob(bytes)
            }
        }
    };
    Some(stored)
}

/// Match the Chronolog `rk1:` format, including invalid UTF-8 stored as TEXT
/// and the exact bits of an IEEE-754 REAL.
pub(crate) unsafe extern "C" fn row_key(
    context: *mut sys::sqlite3_context,
    count: c_int,
    values: *mut *mut sys::sqlite3_value,
) {
    if !(1..=64).contains(&count) {
        sys::sqlite3_result_error(
            context,
            c"chronolog_row_key: expected 1 to 64 values".as_ptr(),
            -1,
        );
        return;
    }
    let connection = sys::sqlite3_context_db_handle(context);
    let limit = sys::sqlite3_limit(connection, sys::SQLITE_LIMIT_LENGTH, -1) as usize;
    let mut key = b"rk1:".to_vec();
    append_hex(&mut key, &(count as u16).to_be_bytes());
    for value in std::slice::from_raw_parts(values, count as usize) {
        let Some(value) = stored_value(*value) else {
            sys::sqlite3_result_error_nomem(context);
            return;
        };
        let extra = value.encoded_size();
        if key.len().saturating_add(extra) > limit {
            sys::sqlite3_result_error_toobig(context);
            return;
        }
        if key.try_reserve(extra).is_err() {
            sys::sqlite3_result_error_nomem(context);
            return;
        }
        value.append(&mut key);
    }
    sys::sqlite3_result_text(
        context,
        key.as_ptr().cast::<c_char>(),
        key.len() as c_int,
        sys::SQLITE_TRANSIENT(),
    );
}

/// Compare storage classes and contents without SQLite collation or coercion.
pub(crate) unsafe extern "C" fn value_equal(
    context: *mut sys::sqlite3_context,
    _count: c_int,
    values: *mut *mut sys::sqlite3_value,
) {
    let left = stored_value(*values);
    let right = stored_value(*values.add(1));
    match (left, right) {
        (Some(a), Some(b)) => sys::sqlite3_result_int(context, c_int::from(a == b)),
        _ => sys::sqlite3_result_error_nomem(context),
    }
}
