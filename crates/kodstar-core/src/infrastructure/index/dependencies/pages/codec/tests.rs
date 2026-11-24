// Purpose: 여러 lookup의 독립성·안정 slot·최대 정수와 손상 거절 검증.
// Pattern: Codec Regression Test.
// Usage: 페이지 codec의 물리 형식 변경에서 실행.
// Related: crates/kodstar-core/src/infrastructure/index/dependencies/pages/codec.rs.

use super::*;

#[test]
fn packed_keys_keep_their_slots_when_other_rows_are_removed() -> Result<()> {
    let mut rows = (0..64)
        .map(|slot| Row {
            slot,
            key: slot as i64 + 1,
            part: 0,
            count: 1,
            payload: vec![2],
        })
        .collect::<Vec<_>>();
    let bytes = encode(0, &rows)?;
    let decoded = decode(0, rows.len(), &bytes)?;
    assert_eq!(
        decoded.iter().map(|r| r.key).collect::<Vec<_>>(),
        (1..=64).collect::<Vec<_>>()
    );
    let ids = decoded
        .iter()
        .map(|r| rowid(7, r.slot))
        .collect::<Result<Vec<_>>>()?;
    rows.retain(|r| r.slot != 1 && r.slot != 31);
    let decoded = decode(0, rows.len(), &encode(0, &rows)?)?;
    for row in decoded {
        assert_eq!(rowid(7, row.slot)?, ids[row.slot]);
    }
    let mut trailing = bytes.clone();
    trailing.push(0);
    assert!(decode(0, 64, &trailing).is_err());
    assert!(decode(0, 63, &bytes).is_err());
    assert!(decode(0, 65, &bytes).is_err());
    Ok(())
}

#[test]
fn maximum_values_and_unselected_corruption_are_checked() -> Result<()> {
    let group = (i64::MAX - 1) / 128;
    let mut payload = Vec::new();
    crate::infrastructure::index::dependencies::codec::push(&mut payload, i64::MAX, 0, true)
        .map_err(|e| rusqlite::Error::UserFunctionError(Box::new(e)))?;
    let row = Row {
        slot: 63,
        key: i64::MAX,
        part: i64::MAX,
        count: 1,
        payload,
    };
    let decoded = decode(group, 1, &encode(group, &[row])?)?;
    assert_eq!((decoded[0].key, decoded[0].part), (i64::MAX, i64::MAX));
    assert_eq!(rowid(i64::MAX / 64, 63)?, i64::MAX);
    assert!(rowid(i64::MAX / 64 + 1, 0).is_err());
    let row = || Row {
        slot: 0,
        key: 1,
        part: 0,
        count: 1,
        payload: vec![2],
    };
    let mut duplicate = row();
    duplicate.slot = 1;
    assert!(encode(0, &[row(), duplicate]).is_err());
    let mut bytes = encode(0, &[row()])?;
    *bytes.last_mut().unwrap() = 0;
    assert!(visit(0, 1, &bytes, |_, _, _, _, _| Ok(())).is_err());
    Ok(())
}
