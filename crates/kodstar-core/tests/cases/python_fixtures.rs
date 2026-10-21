// Purpose: pytest fixture의 가까운 소유·타입·미해결 회귀.
// Pattern: Integration Test.
// Usage: engine 통합 테스트에서 실행.
// Related: crates/kodstar-core/tests/engine.rs, crates/kodstar-core/tests/support/mod.rs.

use crate::support::*;
use kodstar_core::*;

#[test]
fn pytest_ancestor_fixtures_provide_annotated_or_constructor_return_types() -> TestResult {
    let p = Project::new()?;
    p.write("client.py", "class Client:\n    def work(self): pass\n")?;
    p.write("conftest.py","import pytest\nfrom client import Client\n@pytest.fixture\ndef client() -> Client:\n    return Client()\n@pytest.fixture(name='other')\ndef renamed():\n    return Client()\n")?;
    p.write(
        "tests/test_client.py",
        "def test_call(client, other):\n    client.work()\n    other.work()\n",
    )?;
    p.build(false)?;
    let rows = p.query(Query::Deps {
        selector: selector("tests/test_client.py", "test_call"),
    })?;
    assert_eq!(
        relationships(&rows)
            .iter()
            .filter(|e| e.relation == "calls"
                && e.target
                    .as_ref()
                    .is_some_and(|t| t.qualified_name == "Client::work"))
            .map(|e| e.count)
            .sum::<u64>(),
        2,
        "{:?}",
        rows.rows
    );
    Ok(())
}

#[test]
fn pytest_untyped_nearer_fixtures_shadow_ancestors_and_reresolve_incrementally() -> TestResult {
    let p = Project::new()?;
    p.write("conftest.py","import pytest\nclass Client:\n    def work(self): pass\n@pytest.fixture\ndef client() -> Client:\n    return Client()\n")?;
    p.write(
        "tests/conftest.py",
        "import pytest\n@pytest.fixture\ndef client():\n    return None\n",
    )?;
    p.write(
        "tests/test_client.py",
        "def test_call(client):\n    client.work()\n",
    )?;
    p.build(false)?;
    let query = || Query::Deps {
        selector: selector("tests/test_client.py", "test_call"),
    };
    assert!(relationships(&p.query(query())?).is_empty());
    std::fs::remove_file(p.root.join("tests/conftest.py"))?;
    assert_eq!(p.build(false)?.parsed_files, 0);
    let incremental = p.query(query())?;
    assert_eq!(relationships(&incremental).len(), 1);
    p.build(true)?;
    assert_eq!(
        relationships(&incremental)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key,
        relationships(&p.query(query())?)[0]
            .target
            .as_ref()
            .ok_or("target")?
            .logical_key
    );
    Ok(())
}
