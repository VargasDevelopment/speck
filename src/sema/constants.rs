//! Compile-time value semantics shared by constants, global initializers, and
//! early array-length resolution. Callers own name lookup and phase diagnostics;
//! typed evaluation owns array shape and struct field construction.

use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, ConstantValue, Expr, StructDecl, ValueType};
use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone)]
pub(super) struct ConstantDefinition {
    pub(super) ty: ValueType,
    pub(super) init: Expr,
    pub(super) span: Span,
}

mod dependencies;
mod evaluation;
pub(super) use dependencies::{ConstantValues, EvaluationError, Phase};
pub(super) use evaluation::Evaluation;

pub(super) struct ConstantEvaluator<'a> {
    definitions: HashMap<String, ConstantDefinition>,
    globals: &'a HashMap<String, ValueType>,
    structs: &'a HashMap<String, StructDecl>,
    invalid: &'a HashSet<String>,
    cache: ConstantValues,
}

impl<'a> ConstantEvaluator<'a> {
    pub(super) fn new(
        definitions: HashMap<String, ConstantDefinition>,
        globals: &'a HashMap<String, ValueType>,
        structs: &'a HashMap<String, StructDecl>,
        invalid: &'a HashSet<String>,
    ) -> Self {
        Self {
            definitions,
            globals,
            structs,
            invalid,
            cache: ConstantValues::new(),
        }
    }

    pub(super) fn failed(&self, name: &str) -> bool {
        self.cache.failed(name)
    }
    pub(super) fn values(&self) -> &HashMap<String, ConstantValue> {
        self.cache.values()
    }

    pub(super) fn evaluate_named(&mut self, name: &str) -> Result<ConstantValue, Diagnostic> {
        let span = self.definitions[name].span;
        self.cache.evaluate(
            name,
            span,
            &self.definitions,
            Phase::Constant,
            |definition| Evaluation::new(&definition.init, Some(definition.ty.clone())),
            |name, definition, evaluation, cache| {
                if self.invalid.contains(name) {
                    return Err(Diagnostic::new(
                        format!("constant `{name}` has an invalid initializer"),
                        definition.span,
                    )
                    .into());
                }
                let value = evaluation.resume(self.structs, |name, span| {
                    if self.globals.contains_key(name) {
                        Err(Diagnostic::new(
                            format!(
                                "constant expressions cannot reference mutable global `{name}`"
                            ),
                            span,
                        )
                        .into())
                    } else {
                        cache.lookup(name, span)
                    }
                })?;
                validate_constant_type(name, definition, &definition.ty, value).map_err(Into::into)
            },
        )
    }
}

pub(super) fn validate_constant_type(
    name: &str,
    definition: &ConstantDefinition,
    ty: &ValueType,
    value: ConstantValue,
) -> Result<ConstantValue, Diagnostic> {
    if value.ty() == *ty {
        Ok(value)
    } else {
        Err(Diagnostic::new(
            format!(
                "constant `{name}` has declared type `{}`, but its initializer evaluates to `{}`",
                ty.name(),
                value.ty().name(),
            ),
            definition.init.span,
        ))
    }
}

pub(super) fn evaluate_initializer(
    expression: &Expr,
    expected: &ValueType,
    constants: &HashMap<String, ConstantValue>,
    globals: &HashMap<String, ValueType>,
    structs: &HashMap<String, StructDecl>,
) -> Result<ConstantValue, String> {
    let lookup = |name: &str, span: Span| {
        if let Some(value) = constants.get(name).cloned() {
            Ok(value)
        } else if globals.contains_key(name) {
            Err(Diagnostic::new(
                format!("global initializers cannot reference mutable global `{name}`"),
                span,
            ))
        } else {
            Err(Diagnostic::new(
                format!("`{name}` is not a compile-time constant"),
                span,
            ))
        }
    };
    Evaluation::new(expression, Some(expected.clone()))
        .resume(structs, lookup)
        .map_err(|diagnostic| diagnostic.message)
}

pub(super) fn evaluate_expression<E: From<Diagnostic>>(
    expression: &Expr,
    lookup: impl FnMut(&str, Span) -> Result<ConstantValue, E>,
) -> Result<ConstantValue, E> {
    Evaluation::new(expression, None).resume(&HashMap::new(), lookup)
}

fn convert_constant(target: ValueType, source: ConstantValue) -> Result<ConstantValue, String> {
    match (target, source) {
        (ValueType::I32, ConstantValue::I32(value)) => Ok(ConstantValue::I32(value)),
        (ValueType::F32, ConstantValue::F32(value)) => Ok(ConstantValue::F32(value)),
        (ValueType::F32, ConstantValue::I32(value)) => Ok(ConstantValue::F32(value as f32)),
        (ValueType::I32, ConstantValue::F32(value)) => {
            Ok(ConstantValue::I32(safe_f32_to_i32(value)))
        }
        (_, ConstantValue::Bool(_)) => Err("cannot convert `bool` to a numeric type".into()),
        (ValueType::Bool, _) => Err("numeric conversions may only target `i32` or `f32`".into()),
        (ValueType::Array { .. }, _) | (_, ConstantValue::Array { .. }) => {
            Err("arrays cannot be converted to numeric types".into())
        }
        (ValueType::Struct(_), _) | (_, ConstantValue::Struct { .. }) => {
            Err("struct values cannot be converted to numeric types".into())
        }
    }
}

fn safe_f32_to_i32(value: f32) -> i32 {
    if value.is_nan() {
        0
    } else if value >= 2_147_483_648.0_f32 {
        i32::MAX
    } else if value <= -2_147_483_648.0_f32 {
        i32::MIN
    } else {
        value.trunc() as i32
    }
}

fn evaluate_binary(
    left: ConstantValue,
    op: BinaryOp,
    right: ConstantValue,
) -> Result<ConstantValue, &'static str> {
    match (left, right) {
        (ConstantValue::I32(left), ConstantValue::I32(right)) => match op {
            BinaryOp::Add => left
                .checked_add(right)
                .map(ConstantValue::I32)
                .ok_or("constant-expression overflow"),
            BinaryOp::Subtract => left
                .checked_sub(right)
                .map(ConstantValue::I32)
                .ok_or("constant-expression overflow"),
            BinaryOp::Multiply => left
                .checked_mul(right)
                .map(ConstantValue::I32)
                .ok_or("constant-expression overflow"),
            BinaryOp::Divide if right == 0 => Err("division by zero in constant expression"),
            BinaryOp::Divide => left
                .checked_div(right)
                .map(ConstantValue::I32)
                .ok_or("constant-expression overflow"),
            BinaryOp::Remainder if right == 0 => Err("remainder by zero in constant expression"),
            BinaryOp::Remainder => left
                .checked_rem(right)
                .map(ConstantValue::I32)
                .ok_or("constant-expression overflow"),
            BinaryOp::Equal => Ok(ConstantValue::Bool(left == right)),
            BinaryOp::NotEqual => Ok(ConstantValue::Bool(left != right)),
            BinaryOp::Less => Ok(ConstantValue::Bool(left < right)),
            BinaryOp::LessEqual => Ok(ConstantValue::Bool(left <= right)),
            BinaryOp::Greater => Ok(ConstantValue::Bool(left > right)),
            BinaryOp::GreaterEqual => Ok(ConstantValue::Bool(left >= right)),
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => Err("Boolean operator requires `bool`"),
        },
        (ConstantValue::F32(left), ConstantValue::F32(right)) => match op {
            BinaryOp::Remainder => Err("remainder requires `i32` operands"),
            BinaryOp::Divide if right == 0.0 => Err("division by zero in constant expression"),
            BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
                let result = match op {
                    BinaryOp::Add => left + right,
                    BinaryOp::Subtract => left - right,
                    BinaryOp::Multiply => left * right,
                    BinaryOp::Divide => left / right,
                    _ => unreachable!(),
                };
                if result.is_finite() {
                    Ok(ConstantValue::F32(result))
                } else {
                    Err("constant-expression overflow")
                }
            }
            BinaryOp::Equal => Ok(ConstantValue::Bool(left == right)),
            BinaryOp::NotEqual => Ok(ConstantValue::Bool(left != right)),
            BinaryOp::Less => Ok(ConstantValue::Bool(left < right)),
            BinaryOp::LessEqual => Ok(ConstantValue::Bool(left <= right)),
            BinaryOp::Greater => Ok(ConstantValue::Bool(left > right)),
            BinaryOp::GreaterEqual => Ok(ConstantValue::Bool(left >= right)),
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => Err("Boolean operator requires `bool`"),
        },
        (ConstantValue::Bool(left), ConstantValue::Bool(right)) => match op {
            BinaryOp::Equal => Ok(ConstantValue::Bool(left == right)),
            BinaryOp::NotEqual => Ok(ConstantValue::Bool(left != right)),
            BinaryOp::LogicalAnd => Ok(ConstantValue::Bool(left && right)),
            BinaryOp::LogicalOr => Ok(ConstantValue::Bool(left || right)),
            _ => Err("operator is not valid for `bool` constants"),
        },
        _ => Err("constant-expression operands must have the same type"),
    }
}
