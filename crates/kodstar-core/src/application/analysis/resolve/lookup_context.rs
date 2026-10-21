// Purpose: Scoped receiver and type lookup context ownership.
// Pattern: Simple Module.
// Usage: Lexical and typed receiver lookup temporarily select a namespace.
// Related: crates/kodstar-core/src/application/analysis/resolve/lexical.rs, crates/kodstar-core/src/application/analysis/resolve/rust/mod.rs.

use crate::application::analysis::resolve::{Binding, FileContext, Lookup};
use crate::domain::diagnostics::Result;
use crate::domain::languages::Language;
use crate::domain::resolve::namespaces::Namespace;

impl Lookup<'_, '_> {
    pub(super) fn receiver_expression(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        expression: &str,
        depth: usize,
        type_path: bool,
    ) -> Result<()> {
        let previous = (self.namespace, self.receiver);
        self.receiver = true;
        if matches!(
            self.language,
            Language::Rust
                | Language::CommonLisp
                | Language::Php
                | Language::Kotlin
                | Language::Scala
        ) {
            self.namespace = if type_path {
                Namespace::Types
            } else {
                Namespace::Values
            };
            self.receiver = true;
        }
        let result = self.expression(context, scope, position, expression, depth);
        (self.namespace, self.receiver) = previous;
        result
    }
    pub(super) fn type_expression(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        expression: &str,
        depth: usize,
    ) -> Result<()> {
        let previous = (self.namespace, self.receiver);
        self.receiver = true;
        if matches!(
            self.language,
            Language::Rust
                | Language::CommonLisp
                | Language::Php
                | Language::Kotlin
                | Language::Scala
        ) {
            self.namespace = Namespace::Types;
        }
        let result = self.expression(context, scope, position, expression, depth);
        (self.namespace, self.receiver) = previous;
        result
    }

    pub(super) fn receiver_binding(
        &mut self,
        context: &FileContext,
        scope: u32,
        position: usize,
        receiver: &str,
        type_path: bool,
    ) -> Result<Option<Binding>> {
        let previous = self.namespace;
        if type_path || self.language == Language::Php {
            self.namespace = if type_path {
                Namespace::Types
            } else {
                Namespace::Values
            };
        }
        let receiver = if self.language == Language::Php {
            receiver.trim_start_matches('$')
        } else {
            receiver
        };
        let result = self.binding(context, scope, position, receiver);
        self.namespace = previous;
        result
    }
}
