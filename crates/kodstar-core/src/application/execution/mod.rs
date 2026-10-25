// Purpose: 명령별 검증·기본 동기화 정책과 공통 실행 순서.
// Pattern: Template Method with Trait.
// Usage: Engine 공개 명령과 CLI·stdio dispatch가 같은 실행기를 호출.
// Related: crates/kodstar-core/src/application/execution/context.rs, crates/kodstar-core/src/application/execution/commands.rs, crates/kodstar-core/src/application/execution/reads.rs.

mod commands;
pub(crate) mod context;
pub(super) mod history;
mod prepare;
pub(super) mod reads;
mod state;
mod watch;

use super::workspace::ProjectOptions;
use crate::domain::{cancellation::Cancellation, diagnostics::Result, results::Progress};
use crate::infrastructure::cache::CacheStore;
pub use commands::Command;
pub(super) use commands::{Cache, Init, Watch};
use context::{Context, Plan};

pub(super) trait CommandHandler {
    type Output;
    fn validate(&self) -> Result<()> {
        Ok(())
    }
    fn plan(&self) -> Plan<'_> {
        Plan::Project(crate::domain::query::Refresh::Metadata)
    }
    fn progress(&mut self, _event: &Progress) -> Result<()> {
        Ok(())
    }
    fn execute(self, context: &mut Context<'_>, cancel: &Cancellation) -> Result<Self::Output>;
}

pub(super) fn run<C: CommandHandler>(
    store: &CacheStore,
    project: &ProjectOptions,
    mut command: C,
    cancel: &Cancellation,
) -> Result<C::Output> {
    cancel.check()?;
    command.validate()?;
    let mut context = Context::prepare(store, project, command.plan(), cancel)?;
    context.synchronize(cancel, &mut |event| command.progress(event))?;
    command.execute(&mut context, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::analysis::InitOptions;
    use crate::application::engine::Engine;
    use crate::domain::diagnostics::Error;
    use std::fs;

    struct Child;
    impl CommandHandler for Child {
        type Output = i64;
        fn execute(self, context: &mut Context<'_>, _: &Cancellation) -> Result<i64> {
            context
                .required()?
                .report
                .as_ref()
                .map(|report| report.generation)
                .ok_or_else(|| Error::operation("default synchronization was not performed"))
        }
    }
    #[test]
    fn a_child_handler_inherits_initialization_gating_and_incremental_synchronization() -> Result<()>
    {
        let temporary = tempfile::Builder::new()
            .prefix("kodstar-default-command-")
            .tempdir()
            .map_err(|error| Error::caused_by("create fixture directory", error))?;
        let root = temporary.path().join("project");
        fs::create_dir_all(&root).map_err(|error| Error::io("create fixture", &root, error))?;
        let source = root.join("main.rs");
        fs::write(&source, "fn first() {}")
            .map_err(|error| Error::io("write fixture", &source, error))?;
        let engine = Engine::new(temporary.path().join("cache"))?;
        let options = ProjectOptions::new(root);
        let cancel = Cancellation::default();
        assert!(run(&engine.cache, &options, Child, &cancel).is_err());
        let first = engine.init(&options, InitOptions::default(), &cancel, &mut |_| Ok(()))?;
        fs::write(&source, "fn second() {} fn third() {}")
            .map_err(|error| Error::io("change fixture", &source, error))?;
        assert_eq!(
            run(&engine.cache, &options, Child, &cancel)?,
            first.generation + 1
        );
        temporary
            .close()
            .map_err(|error| Error::caused_by("remove fixture directory", error))?;
        Ok(())
    }
}
