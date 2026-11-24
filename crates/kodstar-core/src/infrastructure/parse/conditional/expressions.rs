// Purpose: 전처리 조건의 구조·부분 평가와 분기별 유한한 witness 선택.
// Pattern: Boolean Expression.
// Usage: conditional plan이 실제 코드를 실행하지 않고 분기 투영을 구성.
// Related: crates/kodstar-core/src/infrastructure/parse/conditional/mod.rs.

use crate::domain::cancellation::Cancellation;
use crate::domain::diagnostics::{Error, ErrorKind, Result};
use std::sync::Arc;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub(super) enum Expr {
    Constant(bool),
    Atom(String),
    Not(Arc<Expr>),
    And(Arc<Expr>, Arc<Expr>),
    Or(Arc<Expr>, Arc<Expr>),
}

pub(super) type Guard = Arc<Expr>;
pub(super) type Values = std::collections::BTreeMap<String, bool>;

pub(super) fn and(left: Guard, right: Guard) -> Guard {
    match (left.as_ref(), right.as_ref()) {
        (Expr::Constant(false), _) | (_, Expr::Constant(false)) => Arc::new(Expr::Constant(false)),
        (Expr::Constant(true), _) => right,
        (_, Expr::Constant(true)) => left,
        _ if left == right => left,
        _ => Arc::new(Expr::And(left, right)),
    }
}

pub(super) fn not(value: Guard) -> Guard {
    match value.as_ref() {
        Expr::Constant(value) => Arc::new(Expr::Constant(!value)),
        Expr::Not(inner) => Arc::clone(inner),
        _ => Arc::new(Expr::Not(value)),
    }
}

impl Expr {
    pub(super) fn atoms(&self, values: &mut Values) {
        match self {
            Self::Atom(name) => {
                values.entry(name.clone()).or_insert(false);
            }
            Self::Not(inner) => inner.atoms(values),
            Self::And(left, right) | Self::Or(left, right) => {
                left.atoms(values);
                right.atoms(values);
            }
            Self::Constant(_) => {}
        }
    }
    pub(super) fn evaluate(&self, values: &Values) -> Option<bool> {
        match self {
            Self::Constant(value) => Some(*value),
            Self::Atom(name) => values.get(name).copied(),
            Self::Not(inner) => inner.evaluate(values).map(|v| !v),
            Self::And(a, b) => match (a.evaluate(values), b.evaluate(values)) {
                (Some(false), _) | (_, Some(false)) => Some(false),
                (Some(true), Some(true)) => Some(true),
                _ => None,
            },
            Self::Or(a, b) => match (a.evaluate(values), b.evaluate(values)) {
                (Some(true), _) | (_, Some(true)) => Some(true),
                (Some(false), Some(false)) => Some(false),
                _ => None,
            },
        }
    }

    fn unknown<'a>(&'a self, values: &Values) -> Option<&'a str> {
        match self {
            Self::Atom(name) if !values.contains_key(name) => Some(name),
            Self::Not(inner) => inner.unknown(values),
            Self::And(a, b) | Self::Or(a, b) => a.unknown(values).or_else(|| b.unknown(values)),
            _ => None,
        }
    }

    pub(super) fn witness(&self, cancel: &Cancellation) -> Result<Option<Values>> {
        let mut values = Values::new();
        let mut steps = 0;
        self.search(&mut values, &mut steps, cancel)
            .map(|found| found.then_some(values))
    }

    fn search(
        &self,
        values: &mut Values,
        steps: &mut usize,
        cancel: &Cancellation,
    ) -> Result<bool> {
        *steps += 1;
        if *steps > 16384 || values.len() > 128 {
            return Err(limit());
        }
        cancel.check()?;
        if let Some(value) = self.evaluate(values) {
            return Ok(value);
        }
        let name = self
            .unknown(values)
            .ok_or_else(|| Error::operation("conditional expression has no unresolved atom"))?
            .to_owned();
        for value in [false, true] {
            values.insert(name.clone(), value);
            if self.search(values, steps, cancel)? {
                return Ok(true);
            }
        }
        values.remove(&name);
        Ok(false)
    }

    pub(super) fn describe(&self, output: &mut String, limit_bytes: usize) -> Result<()> {
        match self {
            Self::Constant(value) => output.push_str(if *value { "true" } else { "false" }),
            Self::Atom(name) => output.push_str(name),
            Self::Not(inner) => {
                output.push_str("!(");
                inner.describe(output, limit_bytes)?;
                output.push(')');
            }
            Self::And(a, b) | Self::Or(a, b) => {
                output.push('(');
                a.describe(output, limit_bytes)?;
                output.push_str(if matches!(self, Self::And(..)) {
                    ") && ("
                } else {
                    ") || ("
                });
                b.describe(output, limit_bytes)?;
                output.push(')');
            }
        }
        if output.len() > limit_bytes {
            return Err(limit());
        }
        Ok(())
    }
}

pub(super) fn limit() -> Error {
    Error::new(
        ErrorKind::Limit,
        "conditional compilation expression exceeds its bounded workspace",
    )
}
