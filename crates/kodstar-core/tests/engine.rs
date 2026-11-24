// Purpose: 엔진의 언어·저장·API 통합 사례 모듈 연결.
// Pattern: Test Suite.
// Usage: cargo test -p kodstar-core --test engine.
// Related: crates/kodstar-core/tests/support/mod.rs, crates/kodstar-core/tests/cases/engine_contracts.rs.

#[path = "cases/annotations.rs"]
mod annotations;
#[path = "cases/append_reuse.rs"]
mod append_reuse;
#[path = "cases/astro.rs"]
mod astro;
#[path = "cases/blade.rs"]
mod blade;
#[path = "cases/cache.rs"]
mod cache;
#[path = "cases/callable_values.rs"]
mod callable_values;
#[path = "cases/checkpoint_delta.rs"]
mod checkpoint_delta;
#[path = "cases/clang.rs"]
mod clang;
#[path = "cases/component_links.rs"]
mod component_links;
#[path = "cases/components.rs"]
mod components;
#[path = "cases/configuration_failures.rs"]
mod configuration_failures;
#[path = "cases/configuration_paths.rs"]
mod configuration_paths;
#[path = "cases/consistency.rs"]
mod consistency;
#[path = "cases/coverage.rs"]
mod coverage;
#[path = "cases/cxx.rs"]
mod cxx;
#[path = "cases/cxx_guards.rs"]
mod cxx_guards;
#[path = "cases/cxx_identities.rs"]
mod cxx_identities;
#[path = "cases/declarations.rs"]
mod declarations;
#[path = "cases/dispatch.rs"]
mod dispatch;
#[path = "cases/dotnet_projects.rs"]
mod dotnet_projects;
#[path = "cases/ejs.rs"]
mod ejs;
#[path = "cases/ejs_boundaries.rs"]
mod ejs_boundaries;
#[path = "cases/ejs_scopes.rs"]
mod ejs_scopes;
#[path = "cases/elixir.rs"]
mod elixir;
#[path = "cases/engine_contracts.rs"]
mod engine_contracts;
#[path = "cases/erlang.rs"]
mod erlang;
#[path = "cases/erlang_types.rs"]
mod erlang_types;
#[path = "cases/external_targets.rs"]
mod external_targets;
#[path = "cases/fields.rs"]
mod fields;
#[path = "cases/formats.rs"]
mod formats;
#[path = "cases/getattr_values.rs"]
mod getattr_values;
#[path = "cases/go_incremental.rs"]
mod go_incremental;
#[path = "cases/go_interfaces.rs"]
mod go_interfaces;
#[path = "cases/go_paths.rs"]
mod go_paths;
#[path = "cases/history_alignment.rs"]
mod history_alignment;
#[path = "cases/history_impact.rs"]
mod history_impact;
#[path = "cases/history_metadata.rs"]
mod history_metadata;
#[path = "cases/history_operations.rs"]
mod history_operations;
#[path = "cases/history_paths.rs"]
mod history_paths;
#[path = "cases/history_recovery.rs"]
mod history_recovery;
#[path = "cases/history_streaming.rs"]
mod history_streaming;
#[path = "cases/history_upgrade.rs"]
mod history_upgrade;
#[path = "cases/history_watch.rs"]
mod history_watch;
#[path = "cases/htmx.rs"]
mod htmx;
#[path = "cases/http.rs"]
mod http;
#[path = "cases/http_lookup.rs"]
mod http_lookup;
#[path = "cases/hybrid_recovery.rs"]
mod hybrid_recovery;
#[path = "cases/identities.rs"]
mod identities;
#[path = "cases/incremental_bodies.rs"]
mod incremental_bodies;
#[path = "cases/incremental_hierarchy_bodies.rs"]
mod incremental_hierarchy_bodies;
#[path = "cases/incremental_inputs.rs"]
mod incremental_inputs;
#[path = "cases/incremental_scopes.rs"]
mod incremental_scopes;
#[path = "cases/incremental_statistics.rs"]
mod incremental_statistics;
#[path = "cases/input_contexts.rs"]
mod input_contexts;
#[path = "cases/input_failures.rs"]
mod input_failures;
#[path = "cases/javascript.rs"]
mod javascript;
#[path = "cases/kotlin.rs"]
mod kotlin;
#[path = "cases/kotlin_names.rs"]
mod kotlin_names;
#[path = "cases/languages.rs"]
mod languages;
#[path = "cases/lisp.rs"]
mod lisp;
#[path = "cases/lisp_bindings.rs"]
mod lisp_bindings;
#[path = "cases/lisp_packages.rs"]
mod lisp_packages;
#[path = "cases/local_history.rs"]
mod local_history;
#[path = "cases/module_incremental.rs"]
mod module_incremental;
#[path = "cases/module_rules.rs"]
mod module_rules;
#[path = "cases/namespaces.rs"]
mod namespaces;
#[path = "cases/native_context.rs"]
mod native_context;
#[path = "cases/native_input_budget.rs"]
mod native_input_budget;
#[path = "cases/native_inputs.rs"]
mod native_inputs;
#[path = "cases/native_structures.rs"]
mod native_structures;
#[path = "cases/optimization.rs"]
mod optimization;
#[path = "cases/ownership.rs"]
mod ownership;
#[path = "cases/package_paths.rs"]
mod package_paths;
#[path = "cases/parser_input.rs"]
mod parser_input;
#[path = "cases/parser_macros.rs"]
mod parser_macros;
#[path = "cases/parser_native.rs"]
mod parser_native;
#[path = "cases/php.rs"]
mod php;
#[path = "cases/php_traits.rs"]
mod php_traits;
#[path = "cases/privacy.rs"]
mod privacy;
#[path = "cases/python_comprehensions.rs"]
mod python_comprehensions;
#[path = "cases/python_exports.rs"]
mod python_exports;
#[path = "cases/python_fixtures.rs"]
mod python_fixtures;
#[path = "cases/python_members.rs"]
mod python_members;
#[path = "cases/python_scope.rs"]
mod python_scope;
#[path = "cases/razor.rs"]
mod razor;
#[path = "cases/razor_ranges.rs"]
mod razor_ranges;
#[path = "cases/refresh_reuse.rs"]
mod refresh_reuse;
#[path = "cases/requests.rs"]
mod requests;
#[path = "cases/resolution.rs"]
mod resolution;
#[path = "cases/resources.rs"]
mod resources;
#[path = "cases/return_values.rs"]
mod return_values;
#[path = "cases/ruby.rs"]
mod ruby;
#[path = "cases/ruby_barriers.rs"]
mod ruby_barriers;
#[path = "cases/rust_macros.rs"]
mod rust_macros;
#[path = "cases/rust_namespaces.rs"]
mod rust_namespaces;
#[path = "cases/scala.rs"]
mod scala;
#[path = "cases/source_forms.rs"]
mod source_forms;
#[path = "cases/storage_atoms.rs"]
mod storage_atoms;
#[path = "cases/storage_projection.rs"]
mod storage_projection;
mod support;
#[path = "cases/swift.rs"]
mod swift;
#[path = "cases/swift_packages.rs"]
mod swift_packages;
#[path = "cases/synchronization.rs"]
mod synchronization;
#[path = "cases/type_rules.rs"]
mod type_rules;
#[path = "cases/watch.rs"]
mod watch;
#[path = "cases/zig.rs"]
mod zig;
