//! Demand-driven declaration scheduling. Evaluating a reference can suspend one
//! pure initializer, but never recursively evaluate another declaration.
use std::collections::HashMap;

use crate::ast::ConstantValue;
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

use super::ConstantDefinition;

pub(in crate::sema) enum EvaluationError {
    Diagnostic(Diagnostic),
    Dependency { name: String, span: Span },
}

impl From<Diagnostic> for EvaluationError {
    fn from(diagnostic: Diagnostic) -> Self {
        Self::Diagnostic(diagnostic)
    }
}

#[derive(Clone, Copy)]
pub(in crate::sema) enum Phase {
    Constant,
    ArrayLength,
}

impl Phase {
    fn cycle(self) -> &'static str {
        match self {
            Self::Constant => "cyclic constant definition",
            Self::ArrayLength => "cyclic array-length constant",
        }
    }
    fn unknown(self) -> &'static str {
        match self {
            Self::Constant => "unknown constant",
            Self::ArrayLength => "unknown array-length constant",
        }
    }
}

pub(in crate::sema) struct ConstantValues {
    values: HashMap<String, ConstantValue>,
    failures: HashMap<String, Diagnostic>,
}

impl ConstantValues {
    pub(in crate::sema) fn new() -> Self {
        Self {
            values: builtins::CONSTANTS
                .iter()
                .map(|item| (item.name.to_owned(), item.value.clone()))
                .collect(),
            failures: HashMap::new(),
        }
    }

    pub(in crate::sema) fn values(&self) -> &HashMap<String, ConstantValue> {
        &self.values
    }
    pub(in crate::sema) fn failed(&self, name: &str) -> bool {
        self.failures.contains_key(name)
    }
    pub(in crate::sema) fn into_failed_names(self) -> impl Iterator<Item = String> {
        self.failures.into_keys()
    }

    pub(in crate::sema) fn lookup(
        &self,
        name: &str,
        span: Span,
    ) -> Result<ConstantValue, EvaluationError> {
        if let Some(value) = self.values.get(name) {
            return Ok(value.clone());
        }
        if let Some(diagnostic) = self.failures.get(name) {
            return Err(diagnostic.clone().into());
        }
        Err(EvaluationError::Dependency {
            name: name.to_owned(),
            span,
        })
    }

    pub(in crate::sema) fn evaluate<'a, State>(
        &mut self,
        name: &str,
        span: Span,
        definitions: &'a HashMap<String, ConstantDefinition>,
        phase: Phase,
        mut initialize: impl FnMut(&'a ConstantDefinition) -> State,
        mut initializer: impl FnMut(
            &str,
            &'a ConstantDefinition,
            &mut State,
            &Self,
        ) -> Result<ConstantValue, EvaluationError>,
    ) -> Result<ConstantValue, Diagnostic> {
        match self.lookup(name, span) {
            Ok(value) => return Ok(value),
            Err(EvaluationError::Diagnostic(diagnostic)) => return Err(diagnostic),
            Err(EvaluationError::Dependency { .. }) => {}
        }
        let mut pending = vec![(name.to_owned(), span, None)];
        let mut active = HashMap::from([(name.to_owned(), 0)]);
        loop {
            let (current, usage_span, state) = pending.last_mut().unwrap();
            let result = match definitions.get(current) {
                Some(definition) => initializer(
                    current,
                    definition,
                    state.get_or_insert_with(|| initialize(definition)),
                    self,
                ),
                None => Err(Diagnostic::new(
                    format!("{} `{current}`", phase.unknown()),
                    *usage_span,
                )
                .into()),
            };
            let diagnostic = match result {
                Ok(value) => {
                    self.values.insert(current.clone(), value);
                    active.remove(current);
                    pending.pop();
                    if pending.is_empty() {
                        return Ok(self.values[name].clone());
                    }
                    continue;
                }
                Err(EvaluationError::Dependency { name, span }) => {
                    if let Some(start) = active.get(&name) {
                        let cycle = pending[*start..]
                            .iter()
                            .map(|(name, _, _)| name.as_str())
                            .chain(std::iter::once(name.as_str()))
                            .collect::<Vec<_>>()
                            .join(" -> ");
                        Diagnostic::new(
                            format!("{}: {cycle}", phase.cycle()),
                            definitions[&name].span,
                        )
                    } else {
                        active.insert(name.clone(), pending.len());
                        pending.push((name, span, None));
                        continue;
                    }
                }
                Err(EvaluationError::Diagnostic(diagnostic)) => diagnostic,
            };
            // All suspended initializers depend on this same root failure.
            // Retain it once per declaration so later requests cannot invent
            // derivative cycles or diagnostics with unrelated source locations.
            for (name, _, _) in pending {
                self.failures.insert(name, diagnostic.clone());
            }
            return Err(diagnostic);
        }
    }
}
