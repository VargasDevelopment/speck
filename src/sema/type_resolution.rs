//! Resolve declared array lengths before ordinary semantic checking. This phase
//! can evaluate constants whose own aggregate types still need length resolution.

use std::collections::{HashMap, HashSet};

use crate::ast::{
    ArrayLength, Block, ConstantValue, Expr, Program, ReturnType, StmtKind, StructDecl, ValueType,
};
use crate::diagnostic::{Diagnostic, Span};

use super::constants::{
    ConstantDefinition, ConstantValues, Evaluation, EvaluationError, Phase, validate_constant_type,
};
use super::push_unique_diagnostic;

struct LengthConstantEvaluator {
    definitions: HashMap<String, ConstantDefinition>,
    structs: HashMap<String, StructDecl>,
    cache: ConstantValues,
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
            cache: ConstantValues::new(),
        }
    }

    fn evaluate(&mut self, name: &str, usage_span: Span) -> Result<i32, Diagnostic> {
        require_length_type(name, usage_span, &self.definitions)?;
        let value = self.cache.evaluate(
            name,
            usage_span,
            &self.definitions,
            Phase::ArrayLength,
            |definition| LengthEvaluation::new(definition, &self.structs),
            |name, definition, evaluation, cache| {
                let value = evaluation.resume(&self.definitions, cache)?;
                validate_constant_type(name, definition, &evaluation.types.ty, value)
                    .map_err(Into::into)
            },
        )?;
        length_value(value, name, usage_span)
    }
}

fn require_length_type(
    name: &str,
    span: Span,
    definitions: &HashMap<String, ConstantDefinition>,
) -> Result<(), Diagnostic> {
    if definitions
        .get(name)
        .is_some_and(|definition| definition.ty != ValueType::I32)
    {
        Err(Diagnostic::new(
            format!("array length constant `{name}` must have type `i32`"),
            span,
        ))
    } else {
        Ok(())
    }
}

fn length_value(value: ConstantValue, name: &str, span: Span) -> Result<i32, Diagnostic> {
    match value {
        ConstantValue::I32(value) => Ok(value),
        _ => Err(Diagnostic::new(
            format!("array length constant `{name}` must evaluate to `i32`"),
            span,
        )),
    }
}

struct LengthEvaluation<'a> {
    init: &'a Expr,
    types: PreparedTypes,
    evaluation: Option<Evaluation<'a>>,
}

impl<'a> LengthEvaluation<'a> {
    fn new(definition: &'a ConstantDefinition, structs: &HashMap<String, StructDecl>) -> Self {
        Self {
            init: &definition.init,
            types: PreparedTypes::new(&definition.ty, structs),
            evaluation: None,
        }
    }

    fn resume(
        &mut self,
        definitions: &HashMap<String, ConstantDefinition>,
        cache: &ConstantValues,
    ) -> Result<ConstantValue, EvaluationError> {
        self.types.resume(definitions, cache)?;
        self.evaluation
            .get_or_insert_with(|| Evaluation::new(self.init, Some(self.types.ty.clone())))
            .resume(&self.types.structs, |name, span| cache.lookup(name, span))
    }
}

/// Type preparation retains its own cursor: wide field/length lists must not
/// restart when their initializer later suspends, or when another length is needed.
struct PreparedTypes {
    ty: ValueType,
    structs: HashMap<String, StructDecl>,
    lengths: Vec<ArrayLength>,
    cursor: usize,
    ready: bool,
}

impl PreparedTypes {
    fn new(ty: &ValueType, declarations: &HashMap<String, StructDecl>) -> Self {
        let mut preparation = Self {
            ty: ty.clone(),
            structs: HashMap::new(),
            lengths: Vec::new(),
            cursor: 0,
            ready: false,
        };
        let mut pending = vec![ty.clone()];
        let mut seen = HashSet::new();
        while let Some(ty) = pending.pop() {
            collect_lengths(&ty, &mut preparation.lengths);
            if let Some(name) = struct_name(&ty) {
                if !seen.insert(name.to_owned()) {
                    continue;
                }
                if let Some(declaration) = declarations.get(name) {
                    pending.extend(
                        declaration
                            .fields
                            .iter()
                            .rev()
                            .map(|field| field.ty.clone()),
                    );
                    preparation
                        .structs
                        .insert(name.to_owned(), declaration.clone());
                }
            }
        }
        preparation
    }

    fn resume(
        &mut self,
        definitions: &HashMap<String, ConstantDefinition>,
        cache: &ConstantValues,
    ) -> Result<(), EvaluationError> {
        if self.ready {
            return Ok(());
        }
        let mut constant = |name: &str, span: Span| {
            require_length_type(name, span, definitions)?;
            length_value(cache.lookup(name, span)?, name, span).map_err(Into::into)
        };
        while let Some(length) = self.lengths.get(self.cursor) {
            match length {
                ArrayLength::Literal { value, span } => {
                    positive_array_length(*value, *span)?;
                }
                ArrayLength::Constant { name, span } => {
                    positive_array_length(i64::from(constant(name, *span)?), *span)?;
                }
                ArrayLength::Resolved(_) | ArrayLength::Invalid => {}
            }
            self.cursor += 1;
        }
        // Every demanded length is now cached. Resolve each owned type once.
        resolve_cached_type(&mut self.ty, &mut constant)?;
        for declaration in self.structs.values_mut() {
            for field in &mut declaration.fields {
                resolve_cached_type(&mut field.ty, &mut constant)?;
            }
        }
        self.ready = true;
        Ok(())
    }
}

fn collect_lengths(ty: &ValueType, lengths: &mut Vec<ArrayLength>) {
    if let ValueType::Array { element, length } = ty {
        collect_lengths(element, lengths);
        lengths.push(length.clone());
    }
}

/// Apply already-cached lengths to one bounded syntax type. Preparation has
/// demanded every length before this pass; it never invokes the scheduler.
fn resolve_cached_type(
    ty: &mut ValueType,
    constant: &mut impl FnMut(&str, Span) -> Result<i32, EvaluationError>,
) -> Result<(), EvaluationError> {
    if let ValueType::Array { element, length } = ty {
        resolve_cached_type(element, constant)?;
        *length = match length.clone() {
            ArrayLength::Literal { value, span } => {
                ArrayLength::Resolved(positive_array_length(value, span)?)
            }
            ArrayLength::Constant { name, span } => ArrayLength::Resolved(positive_array_length(
                i64::from(constant(&name, span)?),
                span,
            )?),
            resolved => resolved,
        };
    }
    Ok(())
}

fn struct_name(mut ty: &ValueType) -> Option<&str> {
    while let ValueType::Array { element, .. } = ty {
        ty = element;
    }
    match ty {
        ValueType::Struct(name) => Some(name),
        _ => None,
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
    evaluator.cache.into_failed_names().collect()
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
