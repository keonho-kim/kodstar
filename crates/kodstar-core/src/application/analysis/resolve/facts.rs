// Purpose: 상속과 일반 사실을 id 순서로 순차 해석해 전역 정렬을 제거.
// Pattern: Streaming Resolver.
// Usage: 해석 조정자가 고정 statement·선택 범위·취소를 전달.
// Related: crates/kodstar-core/src/application/analysis/resolve/mod.rs, crates/kodstar-core/src/application/analysis/resolve/prepared.rs.

use super::{FileContext, Lookup, callable_values, context, inputs, prepared};
use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::Result;
use crate::infrastructure::index::cached::Sql as Connection;

pub(super) fn run<'conn>(
    connection: &Connection<'conn>,
    cancel: &Cancellation,
    prepared: &prepared::Prepared<'conn>,
    merged: bool,
    selected_only: bool,
) -> Result<()> {
    let mut current_context: Option<FileContext> = None;
    let filter = if selected_only {
        "(file_id IN (SELECT id FROM changed_files) OR id IN (SELECT id FROM affected_facts)) AND "
    } else {
        ""
    };
    for phase in [
        "relation_id IN (SELECT id FROM atoms WHERE value IN ('inherits','implements','mixes_in'))",
        "relation_id NOT IN (SELECT id FROM atoms WHERE value IN ('inherits','implements','mixes_in'))",
    ] {
        let mut facts = connection.cached_prepare(&format!("SELECT id,file_id,scope_id,expression,relation,start_byte,source_id,language FROM facts WHERE id>?1 AND {filter}{phase}
            AND NOT (language_id IN (SELECT id FROM atoms WHERE value='go') AND relation_id IN (SELECT id FROM atoms WHERE value='implements') AND expression LIKE '@go-interface:%') ORDER BY id LIMIT ?2"))?;
        let budget = (connection.limit(rusqlite::limits::Limit::SQLITE_LIMIT_LENGTH)? as usize
            / 16)
            .clamp(1024, 65536);
        let mut after = 0;
        loop {
            let batch = super::fact_batch::read(&mut facts, after, budget)?;
            if batch.is_empty() {
                break;
            }
            for item in batch {
                cancel.check()?;
                let super::fact_batch::Input {
                    fact,
                    file,
                    scope,
                    expression,
                    relation,
                    position,
                    source,
                    language,
                } = item;
                after = fact;
                let language = language.parse().ok();
                if current_context
                    .as_ref()
                    .is_none_or(|c| c.id != file || Some(c.language) != language)
                {
                    current_context = context(connection, file, language)?;
                }
                let Some(context) = current_context.as_ref() else {
                    continue;
                };
                connection.cached_execute("DELETE FROM resolution_candidates", [])?;
                connection.cached_execute("DELETE FROM resolution_externals", [])?;
                let mut lookup = Lookup {
                    connection,
                    fact: Some(fact),
                    relation: &relation,
                    external_scope_id: 0,
                    scope_sequence: 0,
                    external_present: false,
                    reason: None,
                    rule: "lexical_binding",
                    source_override: None,
                    member_depth: 0,
                    candidate_only: false,
                    language: context.language,
                    namespace: crate::domain::resolve::namespaces::Namespace::usage(
                        context.language,
                        &relation,
                        &expression,
                    ),
                    receiver: false,
                    ruby_side_checked: false,
                    prepared,
                };
                lookup.ejs_context(context, scope)?;
                let value_origin = lookup.callable_value_origin(context, scope, position)?;
                if value_origin == callable_values::ValueOrigin::Unproven {
                    continue;
                }
                let input = lookup.input(context, &expression)?;
                if matches!(input, inputs::Disposition::Discarded) {
                    continue;
                }
                if matches!(input, inputs::Disposition::Unhandled) {
                    if let Some(import) = expression.strip_prefix("@module:") {
                        let (module, name) = import
                            .split_once('#')
                            .map_or((import, None), |(module, name)| (module, Some(name)));
                        lookup.module(context, scope, module, name, 0)?;
                    } else {
                        lookup.expression(context, scope, position, &expression, 0)?;
                    }
                }
                if !lookup.callable_value_targets(value_origin, &expression)? {
                    continue;
                }
                lookup.lisp_method_candidates()?;
                lookup.ruby_dispatch()?;
                lookup.publish(context, scope, source, &expression, merged)?;
            }
        }
        drop(facts);
    }

    Ok(())
}
