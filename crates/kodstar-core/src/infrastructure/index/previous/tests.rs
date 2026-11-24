// Purpose: Reject malformed persisted digests before analysis reuse can consume them.
// Pattern: Storage Contract Test.
// Usage: PreviousFiles unit tests exercise the stored hash boundary.
// Related: crates/kodstar-core/src/infrastructure/index/previous.rs.

use super::*;

#[test]
fn optimization_rejects_malformed_stored_content_hashes() -> Result<()> {
    let c = Connection::open_in_memory()?;
    c.execute_batch(
        "CREATE TABLE files(id INTEGER PRIMARY KEY,path BLOB,facts_hash BLOB,
            environment_hash BLOB,size INTEGER,content_hash BLOB,language TEXT,
            status TEXT,path_module TEXT,path_package TEXT);
         CREATE TABLE bindings(file_id INTEGER,kind TEXT);
         CREATE TABLE diagnostics(file_id INTEGER,code TEXT);
         INSERT INTO files VALUES(1,X'61',X'',X'',1,X'00','go','analyzed','','');",
    )?;
    assert!(PreviousFiles::new(&c)?.get(Path::new("a")).is_err());
    c.execute("UPDATE files SET content_hash=?1", [[7u8; 32]])?;
    assert!(
        PreviousFiles::new(&c)?
            .get(Path::new("a"))?
            .unwrap()
            .fingerprints
            .is_none()
    );
    c.execute(
        "UPDATE files SET facts_hash=?1,environment_hash=X'00'",
        [[3u8; 32]],
    )?;
    assert!(PreviousFiles::new(&c)?.get(Path::new("a")).is_err());
    c.execute("UPDATE files SET environment_hash=?1", [[4u8; 32]])?;
    let previous = PreviousFiles::new(&c)?.get(Path::new("a"))?.unwrap();
    assert_eq!(previous.hash, [7; 32]);
    let fingerprints = previous.fingerprints.unwrap();
    assert_eq!(fingerprints.all, [3; 32]);
    assert_eq!(fingerprints.environment, [4; 32]);
    Ok(())
}
