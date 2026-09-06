//! Resumable pure expression evaluation. Tasks and partial aggregate values stay
//! owned by the initializer while a declaration dependency is being evaluated.
use std::collections::HashMap;

use crate::ast::{BinaryOp, ConstantValue, Expr, ExprKind, StructDecl, UnaryOp, ValueType};
use crate::diagnostic::{Diagnostic, Span};

use super::{convert_constant, evaluate_binary};

pub(in crate::sema) struct Evaluation<'a> {
    tasks: Vec<Task<'a>>,
    values: Vec<ConstantValue>,
}

enum Task<'a> {
    Expression(&'a Expr, Option<ValueType>),
    Array {
        element: ValueType,
        start: usize,
        span: Span,
    },
    Struct {
        name: String,
        fields: Vec<String>,
        start: usize,
        span: Span,
    },
    Index(Span),
    Field {
        name: &'a str,
        span: Span,
    },
    Unary {
        op: UnaryOp,
        span: Span,
    },
    BinaryLeft {
        op: BinaryOp,
        right: &'a Expr,
        span: Span,
    },
    BinaryFinish {
        op: BinaryOp,
        span: Span,
    },
    BooleanRight {
        op: BinaryOp,
        span: Span,
    },
    Conversion {
        target: ValueType,
        span: Span,
    },
}

impl<'a> Evaluation<'a> {
    pub(in crate::sema) fn new(expression: &'a Expr, expected: Option<ValueType>) -> Self {
        Self {
            tasks: vec![Task::Expression(expression, expected)],
            values: Vec::new(),
        }
    }

    pub(in crate::sema) fn resume<E: From<Diagnostic>>(
        &mut self,
        structs: &HashMap<String, StructDecl>,
        mut lookup: impl FnMut(&str, Span) -> Result<ConstantValue, E>,
    ) -> Result<ConstantValue, E> {
        while let Some(task) = self.tasks.pop() {
            match task {
                Task::Expression(expression, expected) => {
                    if let ExprKind::Variable(name) = &expression.kind {
                        match lookup(name, expression.span) {
                            Ok(value) => self.values.push(value),
                            Err(error) => {
                                self.tasks.push(Task::Expression(expression, expected));
                                return Err(error);
                            }
                        }
                    } else {
                        self.expand(expression, expected, structs)?;
                    }
                }
                Task::Array {
                    element,
                    start,
                    span,
                } => {
                    check_aggregate_depth(&self.values[start..], span)?;
                    let elements = self.values.split_off(start);
                    self.values.push(ConstantValue::Array {
                        element_type: Box::new(element),
                        elements,
                    });
                }
                Task::Struct {
                    name,
                    fields,
                    start,
                    span,
                } => {
                    check_aggregate_depth(&self.values[start..], span)?;
                    let values = self.values.split_off(start);
                    self.values.push(ConstantValue::Struct {
                        name,
                        fields: fields.into_iter().zip(values).collect(),
                    });
                }
                Task::Index(span) => {
                    let index = self.pop();
                    let base = self.pop();
                    self.values.push(index_value(base, index, span)?);
                }
                Task::Field { name, span } => {
                    let base = self.pop();
                    let ConstantValue::Struct {
                        name: struct_name,
                        fields,
                    } = base
                    else {
                        return Err(Diagnostic::new("only struct values have fields", span).into());
                    };
                    let value = fields
                        .into_iter()
                        .find_map(|(field, value)| (field == name).then_some(value))
                        .ok_or_else(|| {
                            Diagnostic::new(
                                format!("type `{struct_name}` has no field named `{name}`"),
                                span,
                            )
                        })?;
                    self.values.push(value);
                }
                Task::Unary { op, span } => {
                    let value = self.pop();
                    self.values.push(
                        unary_value(op, value).map_err(|message| Diagnostic::new(message, span))?,
                    );
                }
                Task::Conversion { target, span } => {
                    let value = self.pop();
                    self.values.push(
                        convert_constant(target, value)
                            .map_err(|message| Diagnostic::new(message, span))?,
                    );
                }
                Task::BinaryLeft { op, right, span } => {
                    if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                        let ConstantValue::Bool(left) = self.pop() else {
                            return Err(boolean_error(op, span).into());
                        };
                        if left == (op == BinaryOp::LogicalOr) {
                            self.values.push(ConstantValue::Bool(left));
                        } else {
                            self.tasks.push(Task::BooleanRight { op, span });
                            self.tasks.push(Task::Expression(right, None));
                        }
                    } else {
                        self.tasks.push(Task::BinaryFinish { op, span });
                        self.tasks.push(Task::Expression(right, None));
                    }
                }
                Task::BinaryFinish { op, span } => {
                    let right = self.pop();
                    let left = self.pop();
                    self.values.push(
                        evaluate_binary(left, op, right)
                            .map_err(|message| Diagnostic::new(message, span))?,
                    );
                }
                Task::BooleanRight { op, span } => {
                    if !matches!(self.values.last(), Some(ConstantValue::Bool(_))) {
                        return Err(boolean_error(op, span).into());
                    }
                }
            }
        }
        debug_assert_eq!(self.values.len(), 1);
        Ok(self.pop())
    }

    fn pop(&mut self) -> ConstantValue {
        self.values
            .pop()
            .expect("evaluation tasks balance their operands")
    }

    fn expand(
        &mut self,
        expression: &'a Expr,
        expected: Option<ValueType>,
        structs: &HashMap<String, StructDecl>,
    ) -> Result<(), Diagnostic> {
        let span = expression.span;
        let invalid = |message: &str| Diagnostic::new(message, span);
        match &expression.kind {
            ExprKind::I32(value) => self
                .values
                .push(ConstantValue::I32(i32::try_from(*value).map_err(|_| {
                    invalid("integer literal does not fit in `i32`")
                })?)),
            ExprKind::F32(value) if value.is_finite() => {
                self.values.push(ConstantValue::F32(*value))
            }
            ExprKind::F32(_) => return Err(invalid("floating-point constant must be finite")),
            ExprKind::Bool(value) => self.values.push(ConstantValue::Bool(*value)),
            ExprKind::ArrayLiteral(elements) => {
                let Some((element, length)) = expected.as_ref().and_then(ValueType::resolved_array)
                else {
                    return Err(invalid(
                        "array literal requires an explicit array type annotation",
                    ));
                };
                if elements.len() != length {
                    return Err(invalid(&format!(
                        "expected array length {length}, found {} elements",
                        elements.len()
                    )));
                }
                self.tasks.push(Task::Array {
                    element: element.clone(),
                    start: self.values.len(),
                    span,
                });
                self.tasks.extend(
                    elements
                        .iter()
                        .rev()
                        .map(|expression| Task::Expression(expression, Some(element.clone()))),
                );
            }
            ExprKind::StructLiteral { name, fields } => {
                let Some(ValueType::Struct(expected_name)) = expected else {
                    return Err(invalid(
                        "struct literal requires a matching struct type annotation",
                    ));
                };
                if *name != expected_name {
                    return Err(invalid(&format!(
                        "expected struct `{expected_name}`, found struct literal `{name}`"
                    )));
                }
                let declaration = structs
                    .get(name)
                    .ok_or_else(|| invalid(&format!("unknown struct type `{name}`")))?;
                let mut initializers = HashMap::new();
                for field in fields {
                    initializers.entry(&field.name).or_insert(&field.value);
                }
                let mut children = Vec::with_capacity(declaration.fields.len());
                for field in &declaration.fields {
                    let value = initializers.get(&field.name).ok_or_else(|| {
                        invalid(&format!(
                            "missing initializer for field `{}` of `{name}`",
                            field.name
                        ))
                    })?;
                    children.push(Task::Expression(value, Some(field.ty.clone())));
                }
                self.tasks.push(Task::Struct {
                    name: name.clone(),
                    fields: declaration
                        .fields
                        .iter()
                        .map(|field| field.name.clone())
                        .collect(),
                    start: self.values.len(),
                    span,
                });
                self.tasks.extend(children.into_iter().rev());
            }
            ExprKind::Variable(_) => unreachable!("variable lookup can suspend before expansion"),
            ExprKind::Index { base, index } => {
                self.tasks.push(Task::Index(span));
                self.tasks.push(Task::Expression(index, None));
                self.tasks.push(Task::Expression(base, None));
            }
            ExprKind::Field { base, name, .. } => {
                self.tasks.push(Task::Field { name, span });
                self.tasks.push(Task::Expression(base, None));
            }
            ExprKind::Call { name, .. } => {
                return Err(invalid(&format!(
                    "constant expressions cannot call function `{name}`"
                )));
            }
            ExprKind::Conversion { target, args } => {
                if args.len() != 1 {
                    return Err(invalid(&format!(
                        "conversion to `{}` expects 1 argument, but received {}",
                        target.name(),
                        args.len()
                    )));
                }
                self.tasks.push(Task::Conversion {
                    target: target.clone(),
                    span: args[0].span,
                });
                self.tasks.push(Task::Expression(&args[0], None));
            }
            ExprKind::Unary { op, operand } => {
                if *op == UnaryOp::Negate && matches!(operand.kind, ExprKind::I32(2_147_483_648)) {
                    self.values.push(ConstantValue::I32(i32::MIN));
                } else {
                    self.tasks.push(Task::Unary { op: *op, span });
                    self.tasks.push(Task::Expression(operand, None));
                }
            }
            ExprKind::Binary { left, op, right } => {
                self.tasks.push(Task::BinaryLeft {
                    op: *op,
                    right,
                    span,
                });
                self.tasks.push(Task::Expression(left, None));
            }
        }
        Ok(())
    }
}

// Syntax and import depth do not bound values assembled across declarations.
// Enforce this independent budget before constructing an aggregate parent:
// every value admitted to evaluation storage/cache is safe for recursive clone,
// destruction, and LLVM formatting. Invalid parents are never constructed.
const MAX_CONSTANT_AGGREGATE_DEPTH: usize = 128;

fn check_aggregate_depth(children: &[ConstantValue], span: Span) -> Result<(), Diagnostic> {
    let mut pending = children.iter().map(|value| (value, 1)).collect::<Vec<_>>();
    while let Some((value, parent_depth)) = pending.pop() {
        if matches!(
            value,
            ConstantValue::Array { .. } | ConstantValue::Struct { .. }
        ) {
            if parent_depth == MAX_CONSTANT_AGGREGATE_DEPTH {
                return Err(Diagnostic::new(
                    format!(
                        "resolved constant aggregate nesting exceeds limit of {MAX_CONSTANT_AGGREGATE_DEPTH}"
                    ),
                    span,
                ));
            }
            let depth = parent_depth + 1;
            match value {
                ConstantValue::Array { elements, .. } => {
                    pending.extend(elements.iter().map(|value| (value, depth)))
                }
                ConstantValue::Struct { fields, .. } => {
                    pending.extend(fields.iter().map(|(_, value)| (value, depth)))
                }
                _ => unreachable!(),
            }
        }
    }
    Ok(())
}

fn boolean_error(op: BinaryOp, span: Span) -> Diagnostic {
    Diagnostic::new(
        if op == BinaryOp::LogicalAnd {
            "`&&` requires `bool` constants"
        } else {
            "`||` requires `bool` constants"
        },
        span,
    )
}

fn unary_value(op: UnaryOp, value: ConstantValue) -> Result<ConstantValue, &'static str> {
    match (op, value) {
        (UnaryOp::Negate, ConstantValue::I32(value)) => value
            .checked_neg()
            .map(ConstantValue::I32)
            .ok_or("constant-expression overflow"),
        (UnaryOp::Negate, ConstantValue::F32(value)) if value.is_finite() => {
            Ok(ConstantValue::F32(-value))
        }
        (UnaryOp::Negate, ConstantValue::F32(_)) => Err("constant-expression overflow"),
        (UnaryOp::Not, ConstantValue::Bool(value)) => Ok(ConstantValue::Bool(!value)),
        (UnaryOp::Negate, _) => Err("unary `-` requires a numeric constant"),
        (UnaryOp::Not, _) => Err("unary `!` requires `bool`"),
    }
}

fn index_value(
    base: ConstantValue,
    index: ConstantValue,
    span: Span,
) -> Result<ConstantValue, Diagnostic> {
    let ConstantValue::I32(index) = index else {
        return Err(Diagnostic::new("array index must be i32", span));
    };
    let ConstantValue::Array { elements, .. } = base else {
        return Err(Diagnostic::new("only arrays can be indexed", span));
    };
    let length = elements.len();
    let value = usize::try_from(index)
        .ok()
        .and_then(|index| elements.into_iter().nth(index));
    value.ok_or_else(|| {
        Diagnostic::new(
            format!("constant index {index} is out of bounds for length {length}"),
            span,
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::ArrayLength;
    use crate::sema::constants::EvaluationError;

    #[test]
    fn wide_forward_dependencies_do_not_replay_completed_elements() {
        for count in [1000, 4000, 8000] {
            let span = Span::new(0, 1);
            let expression = Expr {
                span,
                kind: ExprKind::ArrayLiteral(
                    (0..count)
                        .map(|index| Expr {
                            span,
                            kind: ExprKind::Binary {
                                left: Box::new(Expr {
                                    span,
                                    kind: ExprKind::I32(index as i64),
                                }),
                                op: BinaryOp::Add,
                                right: Box::new(Expr {
                                    span,
                                    kind: ExprKind::Variable(format!("C{index}")),
                                }),
                            },
                        })
                        .collect(),
                ),
            };
            let expected = ValueType::Array {
                element: Box::new(ValueType::I32),
                length: ArrayLength::Resolved(count),
            };
            let mut evaluation = Evaluation::new(&expression, Some(expected));
            let mut values = HashMap::new();
            let mut lookups = 0;
            let result = loop {
                match evaluation.resume(&HashMap::new(), |name, span| {
                    lookups += 1;
                    values
                        .get(name)
                        .cloned()
                        .ok_or_else(|| EvaluationError::Dependency {
                            name: name.to_owned(),
                            span,
                        })
                }) {
                    Ok(value) => break value,
                    Err(EvaluationError::Dependency { name, .. }) => {
                        let index = name[1..].parse::<i32>().unwrap();
                        values.insert(name, ConstantValue::I32(index));
                    }
                    Err(EvaluationError::Diagnostic(error)) => panic!("{error:?}"),
                }
            };
            // Each reference is attempted once before, and once after, caching.
            // A retry of an aggregate prefix would make this count quadratic.
            assert_eq!(lookups, count * 2);
            let ConstantValue::Array { elements, .. } = result else {
                panic!("expected array");
            };
            assert_eq!(
                elements,
                (0..count)
                    .map(|index| ConstantValue::I32(index as i32 * 2))
                    .collect::<Vec<_>>()
            );
        }
    }
}
