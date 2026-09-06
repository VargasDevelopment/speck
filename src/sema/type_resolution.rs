//! Resolve declared array lengths before ordinary semantic checking. This phase
//! can evaluate constants whose own aggregate types still need length resolution.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    ArrayLength, Block, ConstantValue, Program, ReturnType, StmtKind, StructDecl, ValueType,
};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

use super::constants::{ConstantDefinition, evaluate_expression, evaluate_typed_expression};
use super::push_unique_diagnostic;

struct LengthConstantEvaluator {
    definitions: HashMap<String, ConstantDefinition>,
    structs: HashMap<String, StructDecl>,
    values: HashMap<String, ConstantValue>,
    failures: HashMap<String, Diagnostic>,
    visiting: Vec<String>,
}

impl LengthConstantEvaluator {
    fn new(program: &Program) -> Self {
        Self {
            definitions: program
                .constants
                .iter()
                .map(|constant| {
                    (
                        constant.name.clone(),
                        ConstantDefinition {
                            ty: constant.ty.clone(),
                            init: constant.init.clone(),
                            span: constant.span,
                        },
                    )
                })
                .collect(),
            structs: program
                .structs
                .iter()
                .map(|declaration| (declaration.name.clone(), declaration.clone()))
                .collect(),
            values: builtins::CONSTANTS
                .iter()
                .map(|constant| (constant.name.to_owned(), constant.value.clone()))
                .collect(),
            failures: HashMap::new(),
            visiting: Vec::new(),
        }
    }

    fn evaluate(&mut self, name: &str, usage_span: Span) -> Result<i32, Diagnostic> {
        if self
            .definitions
            .get(name)
            .is_some_and(|definition| definition.ty != ValueType::I32)
        {
            return Err(Diagnostic::new(
                format!("array length constant `{name}` must have type `i32`"),
                usage_span,
            ));
        }
        match self.evaluate_value(name, usage_span)? {
            ConstantValue::I32(value) => Ok(value),
            _ => Err(Diagnostic::new(
                format!("array length constant `{name}` must evaluate to `i32`"),
                usage_span,
            )),
        }
    }

    fn evaluate_value(
        &mut self,
        name: &str,
        usage_span: Span,
    ) -> Result<ConstantValue, Diagnostic> {
        if let Some(value) = self.values.get(name) {
            return Ok(value.clone());
        }
        if let Some(diagnostic) = self.failures.get(name) {
            return Err(diagnostic.clone());
        }
        let Some(definition) = self.definitions.get(name).cloned() else {
            return Err(Diagnostic::new(
                format!("unknown array-length constant `{name}`"),
                usage_span,
            ));
        };
        if let Some(start) = self.visiting.iter().position(|item| item == name) {
            let mut cycle = self.visiting[start..].to_vec();
            cycle.push(name.to_owned());
            return Err(Diagnostic::new(
                format!("cyclic array-length constant: {}", cycle.join(" -> ")),
                definition.span,
            ));
        }

        self.visiting.push(name.to_owned());
        let result = self.evaluate_definition(name, &definition);
        let popped = self.visiting.pop();
        debug_assert_eq!(popped.as_deref(), Some(name));
        let value = match result {
            Ok(value) => value,
            Err(diagnostic) => {
                self.failures.insert(name.to_owned(), diagnostic.clone());
                return Err(diagnostic);
            }
        };
        self.values.insert(name.to_owned(), value.clone());
        Ok(value)
    }

    fn evaluate_definition(
        &mut self,
        name: &str,
        definition: &ConstantDefinition,
    ) -> Result<ConstantValue, Diagnostic> {
        let mut ty = definition.ty.clone();
        let mut diagnostics = Vec::new();
        resolve_value_type(&mut ty, self, &mut diagnostics);
        let structs = self.resolve_structs_for_type(&ty, &mut diagnostics);
        if let Some(diagnostic) = diagnostics.into_iter().next() {
            return Err(diagnostic);
        }
        let value = evaluate_typed_expression(&definition.init, &ty, &structs, |expression| {
            evaluate_expression(expression, |name| {
                self.evaluate_value(name, definition.init.span)
            })
        })?;
        if value.ty() != ty {
            return Err(Diagnostic::new(
                format!(
                    "constant `{name}` has declared type `{}`, but its initializer evaluates to `{}`",
                    ty.name(),
                    value.ty().name()
                ),
                definition.init.span,
            ));
        }
        Ok(value)
    }

    fn resolve_structs_for_type(
        &mut self,
        ty: &ValueType,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> HashMap<String, StructDecl> {
        let mut structs = self.structs.clone();
        let mut resolving = HashSet::new();
        let mut resolved = HashSet::new();
        self.resolve_structs_referenced_by(
            ty,
            &mut structs,
            &mut resolving,
            &mut resolved,
            diagnostics,
        );
        structs
    }

    fn resolve_structs_referenced_by(
        &mut self,
        ty: &ValueType,
        structs: &mut HashMap<String, StructDecl>,
        resolving: &mut HashSet<String>,
        resolved: &mut HashSet<String>,
        diagnostics: &mut Vec<Diagnostic>,
    ) {
        match ty {
            ValueType::Struct(name) => {
                if resolved.contains(name) || !resolving.insert(name.clone()) {
                    return;
                }
                let Some(mut declaration) = structs.get(name).cloned() else {
                    resolving.remove(name);
                    return;
                };
                for field in &mut declaration.fields {
                    resolve_value_type(&mut field.ty, self, diagnostics);
                    self.resolve_structs_referenced_by(
                        &field.ty,
                        structs,
                        resolving,
                        resolved,
                        diagnostics,
                    );
                }
                structs.insert(name.clone(), declaration);
                resolving.remove(name);
                resolved.insert(name.clone());
            }
            ValueType::Array { element, .. } => self.resolve_structs_referenced_by(
                element,
                structs,
                resolving,
                resolved,
                diagnostics,
            ),
            ValueType::I32 | ValueType::F32 | ValueType::Bool => {}
        }
    }
}

pub(super) fn resolve_program_types(
    program: &mut Program,
    diagnostics: &mut Vec<Diagnostic>,
) -> HashSet<String> {
    let snapshot = program.clone();
    let mut evaluator = LengthConstantEvaluator::new(&snapshot);

    for declaration in &mut program.structs {
        for field in &mut declaration.fields {
            resolve_value_type(&mut field.ty, &mut evaluator, diagnostics);
        }
    }
    for constant in &mut program.constants {
        resolve_value_type(&mut constant.ty, &mut evaluator, diagnostics);
    }
    for global in &mut program.globals {
        resolve_value_type(&mut global.ty, &mut evaluator, diagnostics);
    }
    for function in &mut program.functions {
        for param in &mut function.params {
            resolve_value_type(&mut param.ty, &mut evaluator, diagnostics);
        }
        if let ReturnType::Value(ty) = &mut function.return_type {
            resolve_value_type(ty, &mut evaluator, diagnostics);
        }
        resolve_block_types(&mut function.body, &mut evaluator, diagnostics);
    }
    evaluator.failures.into_keys().collect()
}

fn resolve_block_types(
    block: &mut Block,
    evaluator: &mut LengthConstantEvaluator,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in block {
        match &mut statement.kind {
            StmtKind::Let { ty, .. } => resolve_value_type(ty, evaluator, diagnostics),
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                resolve_block_types(then_block, evaluator, diagnostics);
                if let Some(else_block) = else_block {
                    resolve_block_types(else_block, evaluator, diagnostics);
                }
            }
            StmtKind::While { body, .. } => {
                resolve_block_types(body, evaluator, diagnostics);
            }
            StmtKind::For { body, .. } => {
                resolve_block_types(body, evaluator, diagnostics);
            }
            StmtKind::Assign { .. } | StmtKind::Expr(_) | StmtKind::Return(_) => {}
        }
    }
}

fn resolve_value_type(
    ty: &mut ValueType,
    evaluator: &mut LengthConstantEvaluator,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let ValueType::Array { element, length } = ty else {
        return;
    };
    resolve_value_type(element, evaluator, diagnostics);
    let result = match length.clone() {
        ArrayLength::Literal { value, span } => {
            positive_array_length(value, span).map(ArrayLength::Resolved)
        }
        ArrayLength::Constant { name, span } => evaluator
            .evaluate(&name, span)
            .and_then(|value| positive_array_length(i64::from(value), span))
            .map(ArrayLength::Resolved),
        ArrayLength::Resolved(value) => Ok(ArrayLength::Resolved(value)),
        ArrayLength::Invalid => Ok(ArrayLength::Invalid),
    };
    match result {
        Ok(resolved) => *length = resolved,
        Err(diagnostic) => {
            push_unique_diagnostic(diagnostics, diagnostic);
            *length = ArrayLength::Invalid;
        }
    }
}

fn positive_array_length(value: i64, span: Span) -> Result<usize, Diagnostic> {
    if value <= 0 {
        return Err(Diagnostic::new(
            format!("array length must be positive, found {value}"),
            span,
        ));
    }
    if value > i64::from(i32::MAX) {
        return Err(Diagnostic::new(
            "array length exceeds the maximum i32 index range",
            span,
        ));
    }
    usize::try_from(value).map_err(|_| Diagnostic::new("array length is too large", span))
}
