// Purpose: 제외 이벤트 필터가 rename·overflow·불명 경로의 재스캔을 보존하는지 검증.
// Pattern: Boundary Regression Test.
// Usage: watch adapter가 native callback과 같은 순수 판정을 검사.
// Related: crates/kodstar-core/src/infrastructure/watch.rs, crates/kodstar-core/src/infrastructure/scan/policy.rs.

use super::*;
use notify::event::{Flag, ModifyKind, RenameMode};

#[test]
fn excluded_events_do_not_hide_overflow_or_renames_into_source() -> Result<()> {
    let policy = Policy::new(
        Path::new("/project"),
        Path::new("/cache"),
        &["generated".into()],
    )?;
    let ignored =
        Event::new(EventKind::Modify(ModifyKind::Any)).add_path("/project/target/log".into());
    assert!(!relevant(&ignored, &policy));
    assert!(relevant(&ignored.clone().set_flag(Flag::Rescan), &policy));
    let rename = Event::new(EventKind::Modify(ModifyKind::Name(RenameMode::Both)))
        .add_path("/project/generated/api.rs".into())
        .add_path("/project/src/api.rs".into());
    assert!(relevant(&rename, &policy));
    assert!(relevant(&Event::new(EventKind::Any), &policy));
    assert!(relevant(
        &Event::new(EventKind::Modify(ModifyKind::Any)).add_path("/project/.gitignore".into()),
        &policy
    ));
    Ok(())
}
