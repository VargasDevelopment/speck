//! Compile-time value semantics shared by constants, global initializers, and
//! early array-length resolution. Callers own name lookup and phase diagnostics;
//! typed evaluation owns array shape and struct field construction.

use std::collections::{HashMap, HashSet};

use crate::ast::{BinaryOp, ConstantValue, Expr, ExprKind, StructDecl, UnaryOp, ValueType};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone)]
pub(super) struct ConstantDefinition {
    pub(super) ty: ValueType,
    pub(super) init: Expr,
    pub(super) span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
    Failed,
}

pub(super) struct ConstantEvaluator<'a> {
    definitions: HashMap<String, ConstantDefinition>,
    globals: &'a HashMap<String, ValueType>,
    structs: &'a HashMap<String, StructDecl>,
    invalid: &'a HashSet<String>,
    states: HashMap<String, VisitState>,
    pub(super) values: HashMap<String, ConstantValue>,
    stack: Vec<String>,
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
            states: HashMap::new(),
            values: builtins::CONSTANTS
                .iter()
                .map(|constant| (constant.name.to_owned(), constant.value.clone()))
                .collect(),
            stack: Vec::new(),
        }
    }

    pub(super) fn failed(&self, name: &str) -> bool {
        matches!(self.states.get(name), Some(VisitState::Failed))
    }

    pub(super) fn evaluate_named(&mut self, name: &str) -> Result<ConstantValue, Diagnostic> {
        if let Some(value) = self.values.get(name).cloned() {
            return Ok(value);
        }
        let Some(definition) = self.definitions.get(name).cloned() else {
            return Err(Diagnostic::new(
                format!("unknown constant `{name}`"),
                Span::new(0, 0),
            ));
        };
        if self.invalid.contains(name) {
            return Err(Diagnostic::new(
                format!("constant `{name}` has an invalid initializer"),
                definition.span,
            ));
        }
        match self.states.get(name) {
            Some(VisitState::Visiting) => {
                let start = self.stack.iter().position(|item| item == name).unwrap_or(0);
                let mut cycle = self.stack[start..].to_vec();
                cycle.push(name.to_owned());
                return Err(Diagnostic::new(
                    format!("cyclic constant definition: {}", cycle.join(" -> ")),
                    definition.span,
                ));
            }
            Some(VisitState::Failed) => {
                return Err(Diagnostic::new(
                    format!("constant `{name}` depends on an invalid constant"),
                    definition.span,
                ));
            }
            Some(VisitState::Complete) => unreachable!("complete constants have values"),
            None => {}
        }

        self.states.insert(name.to_owned(), VisitState::Visiting);
        self.stack.push(name.to_owned());
        let structs = self.structs;
        let result = evaluate_typed_expression(
            &definition.init,
            &definition.ty,
            structs,
            |expression| self.evaluate_expr(expression),
        )
            .and_then(|value| {
            if value.ty() == definition.ty {
                Ok(value)
            } else {
                Err(Diagnostic::new(
                    format!(
                        "constant `{name}` has declared type `{}`, but its initializer evaluates to `{}`",
                        definition.ty.name(),
                        value.ty().name()
                    ),
                    definition.init.span,
                ))
            }
            });
        self.stack.pop();
        match result {
            Ok(value) => {
                self.states.insert(name.to_owned(), VisitState::Complete);
                self.values.insert(name.to_owned(), value.clone());
                Ok(value)
            }
            Err(error) => {
                self.states.insert(name.to_owned(), VisitState::Failed);
                Err(error)
            }
        }
    }

    fn evaluate_expr(&mut self, expression: &Expr) -> Result<ConstantValue, Diagnostic> {
        match &expression.kind {
            ExprKind::Variable(name) if self.values.contains_key(name) => {
                Ok(self.values[name].clone())
            }
            ExprKind::Variable(name) if self.definitions.contains_key(name) => {
                self.evaluate_named(name)
            }
            ExprKind::Variable(name) if self.globals.contains_key(name) => Err(Diagnostic::new(
                format!("constant expressions cannot reference mutable global `{name}`"),
                expression.span,
            )),
            ExprKind::Variable(name) => Err(Diagnostic::new(
                format!("`{name}` is not a compile-time constant"),
                expression.span,
            )),
            ExprKind::Call { name, .. } => Err(Diagnostic::new(
                format!("constant expressions cannot call function `{name}`"),
                expression.span,
            )),
            _ => evaluate_expression(expression, |name| {
                if self.globals.contains_key(name) {
                    Err(Diagnostic::new(
                        format!("constant expressions cannot reference mutable global `{name}`"),
                        expression.span,
                    ))
                } else {
                    self.evaluate_named(name)
                }
            }),
        }
    }
}

pub(super) fn evaluate_initializer(
    expression: &Expr,
    expected: &ValueType,
    constants: &HashMap<String, ConstantValue>,
    globals: &HashMap<String, ValueType>,
    structs: &HashMap<String, StructDecl>,
) -> Result<ConstantValue, String> {
    let mut lookup = |name: &str| {
        if let Some(value) = constants.get(name).cloned() {
            Ok(value)
        } else if globals.contains_key(name) {
            Err(Diagnostic::new(
                format!("global initializers cannot reference mutable global `{name}`"),
                expression.span,
            ))
        } else {
            Err(Diagnostic::new(
                format!("`{name}` is not a compile-time constant"),
                expression.span,
            ))
        }
    };
    evaluate_typed_expression(expression, expected, structs, |expression| {
        evaluate_expression(expression, &mut lookup)
    })
    .map_err(|diagnostic| diagnostic.message)
}

pub(super) fn evaluate_typed_expression<F>(
    expression: &Expr,
    expected: &ValueType,
    structs: &HashMap<String, StructDecl>,
    mut leaf: F,
) -> Result<ConstantValue, Diagnostic>
where
    F: FnMut(&Expr) -> Result<ConstantValue, Diagnostic>,
{
    fn evaluate<F>(
        expression: &Expr,
        expected: &ValueType,
        structs: &HashMap<String, StructDecl>,
        leaf: &mut F,
    ) -> Result<ConstantValue, Diagnostic>
    where
        F: FnMut(&Expr) -> Result<ConstantValue, Diagnostic>,
    {
        if let ExprKind::StructLiteral {
            name,
            fields: initializers,
        } = &expression.kind
        {
            let ValueType::Struct(expected_name) = expected else {
                return Err(Diagnostic::new(
                    "struct literal requires a matching struct type annotation",
                    expression.span,
                ));
            };
            if name != expected_name {
                return Err(Diagnostic::new(
                    format!("expected struct `{expected_name}`, found struct literal `{name}`"),
                    expression.span,
                ));
            }
            let declaration = structs.get(name).ok_or_else(|| {
                Diagnostic::new(format!("unknown struct type `{name}`"), expression.span)
            })?;
            let mut fields = Vec::with_capacity(declaration.fields.len());
            for field in &declaration.fields {
                let initializer = initializers
                    .iter()
                    .find(|initializer| initializer.name == field.name)
                    .ok_or_else(|| {
                        Diagnostic::new(
                            format!("missing initializer for field `{}` of `{name}`", field.name),
                            expression.span,
                        )
                    })?;
                fields.push((
                    field.name.clone(),
                    evaluate(&initializer.value, &field.ty, structs, leaf)?,
                ));
            }
            return Ok(ConstantValue::Struct {
                name: name.clone(),
                fields,
            });
        }
        if let ExprKind::ArrayLiteral(elements) = &expression.kind {
            let Some((element_type, length)) = expected.resolved_array() else {
                return Err(Diagnostic::new(
                    "array literal requires an array type annotation",
                    expression.span,
                ));
            };
            if elements.len() != length {
                return Err(Diagnostic::new(
                    format!(
                        "expected array length {length}, found {} elements",
                        elements.len()
                    ),
                    expression.span,
                ));
            }
            let mut values = Vec::with_capacity(elements.len());
            for element in elements {
                values.push(evaluate(element, element_type, structs, leaf)?);
            }
            return Ok(ConstantValue::Array {
                element_type: Box::new(element_type.clone()),
                elements: values,
            });
        }
        leaf(expression)
    }

    evaluate(expression, expected, structs, &mut leaf)
}

pub(super) fn evaluate_expression<F>(
    expression: &Expr,
    mut constant: F,
) -> Result<ConstantValue, Diagnostic>
where
    F: FnMut(&str) -> Result<ConstantValue, Diagnostic>,
{
    fn evaluate<F>(expression: &Expr, constant: &mut F) -> Result<ConstantValue, Diagnostic>
    where
        F: FnMut(&str) -> Result<ConstantValue, Diagnostic>,
    {
        let invalid = |message: String| Diagnostic::new(message, expression.span);
        match &expression.kind {
            ExprKind::I32(value) => i32::try_from(*value)
                .map(ConstantValue::I32)
                .map_err(|_| invalid("integer literal does not fit in `i32`".into())),
            ExprKind::F32(value) if value.is_finite() => Ok(ConstantValue::F32(*value)),
            ExprKind::F32(_) => Err(invalid("floating-point constant must be finite".into())),
            ExprKind::Bool(value) => Ok(ConstantValue::Bool(*value)),
            ExprKind::ArrayLiteral(_) => Err(invalid(
                "array literal requires an explicit array type annotation".into(),
            )),
            ExprKind::StructLiteral { name, .. } => Err(invalid(format!(
                "struct literal `{name}` requires its declared struct type"
            ))),
            ExprKind::Variable(name) => constant(name),
            ExprKind::Index { base, index } => {
                let base = evaluate(base, constant)?;
                let index = match evaluate(index, constant)? {
                    ConstantValue::I32(value) => value,
                    _ => return Err(invalid("array index must be i32".into())),
                };
                let ConstantValue::Array { elements, .. } = base else {
                    return Err(invalid("only arrays can be indexed".into()));
                };
                let Ok(index) = usize::try_from(index) else {
                    return Err(invalid(format!(
                        "constant index {index} is out of bounds for length {}",
                        elements.len()
                    )));
                };
                elements.get(index).cloned().ok_or_else(|| {
                    invalid(format!(
                        "constant index {index} is out of bounds for length {}",
                        elements.len()
                    ))
                })
            }
            ExprKind::Field { base, name, .. } => {
                let base = evaluate(base, constant)?;
                let ConstantValue::Struct {
                    name: struct_name,
                    fields,
                } = base
                else {
                    return Err(invalid("only struct values have fields".into()));
                };
                fields
                    .into_iter()
                    .find_map(|(field_name, value)| (field_name == *name).then_some(value))
                    .ok_or_else(|| {
                        invalid(format!("type `{struct_name}` has no field named `{name}`"))
                    })
            }
            ExprKind::Call { name, .. } => Err(invalid(format!(
                "constant expressions cannot call function `{name}`"
            ))),
            ExprKind::Conversion { target, args } => {
                if args.len() != 1 {
                    return Err(invalid(format!(
                        "conversion to `{}` expects 1 argument, but received {}",
                        target.name(),
                        args.len()
                    )));
                }
                let source = evaluate(&args[0], constant)?;
                convert_constant(target.clone(), source)
                    .map_err(|message| Diagnostic::new(message, args[0].span))
            }
            ExprKind::Unary { op, operand } => {
                if *op == UnaryOp::Negate
                    && matches!(operand.kind, ExprKind::I32(value) if value == 2_147_483_648)
                {
                    return Ok(ConstantValue::I32(i32::MIN));
                }
                let value = evaluate(operand, constant)?;
                match (op, value) {
                    (UnaryOp::Negate, ConstantValue::I32(value)) => value
                        .checked_neg()
                        .map(ConstantValue::I32)
                        .ok_or_else(|| invalid("constant-expression overflow".into())),
                    (UnaryOp::Negate, ConstantValue::F32(value)) => {
                        let result = -value;
                        if result.is_finite() {
                            Ok(ConstantValue::F32(result))
                        } else {
                            Err(invalid("constant-expression overflow".into()))
                        }
                    }
                    (UnaryOp::Not, ConstantValue::Bool(value)) => Ok(ConstantValue::Bool(!value)),
                    (UnaryOp::Negate, _) => {
                        Err(invalid("unary `-` requires a numeric constant".into()))
                    }
                    (UnaryOp::Not, _) => Err(invalid("unary `!` requires `bool`".into())),
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left = evaluate(left, constant)?;
                if *op == BinaryOp::LogicalAnd {
                    return match left {
                        ConstantValue::Bool(false) => Ok(ConstantValue::Bool(false)),
                        ConstantValue::Bool(true) => match evaluate(right, constant)? {
                            ConstantValue::Bool(value) => Ok(ConstantValue::Bool(value)),
                            _ => Err(invalid("`&&` requires `bool` constants".into())),
                        },
                        _ => Err(invalid("`&&` requires `bool` constants".into())),
                    };
                }
                if *op == BinaryOp::LogicalOr {
                    return match left {
                        ConstantValue::Bool(true) => Ok(ConstantValue::Bool(true)),
                        ConstantValue::Bool(false) => match evaluate(right, constant)? {
                            ConstantValue::Bool(value) => Ok(ConstantValue::Bool(value)),
                            _ => Err(invalid("`||` requires `bool` constants".into())),
                        },
                        _ => Err(invalid("`||` requires `bool` constants".into())),
                    };
                }
                let right = evaluate(right, constant)?;
                evaluate_binary(left, *op, right).map_err(|message| invalid(message.to_owned()))
            }
        }
    }

    evaluate(expression, &mut constant)
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
