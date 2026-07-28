use std::collections::{HashMap, HashSet};

use crate::ast::{
    ArrayLength, AssignOp, BinaryOp, Block, ConstantValue, Expr, ExprKind, Function, FunctionKind,
    Program, ReturnType, Stmt, StmtKind, StructDecl, UnaryOp, ValueType,
};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone)]
struct Signature {
    params: Vec<ValueType>,
    return_type: ReturnType,
}

#[derive(Clone)]
struct Binding {
    ty: ValueType,
    mutability: Mutability,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mutability {
    Mutable,
    Constant,
    LoopVariable,
}

pub fn check(program: &mut Program) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let invalid_length_constants = resolve_program_types(program, &mut diagnostics);
    let mut constants = builtins::CONSTANTS
        .iter()
        .map(|constant| (constant.name.to_owned(), constant.value.ty()))
        .collect::<HashMap<_, _>>();
    let mut globals = HashMap::new();
    let mut top_level_names: HashMap<String, &'static str> = builtins()
        .into_keys()
        .map(|name| (name, "function"))
        .collect();
    top_level_names.extend(
        builtins::CONSTANTS
            .iter()
            .map(|constant| (constant.name.to_owned(), "predefined constant")),
    );
    let mut structs = HashMap::new();
    for declaration in &program.structs {
        let duplicate = structs
            .insert(declaration.name.clone(), declaration.clone())
            .is_some();
        if duplicate {
            diagnostics.push(Diagnostic::new(
                format!("struct `{}` is declared more than once", declaration.name),
                declaration.span,
            ));
        } else if let Some(existing) = top_level_names.insert(declaration.name.clone(), "struct") {
            diagnostics.push(Diagnostic::new(
                format!(
                    "struct `{}` conflicts with an existing {existing}",
                    declaration.name
                ),
                declaration.span,
            ));
        }
        let mut fields = HashSet::new();
        for field in &declaration.fields {
            if !fields.insert(&field.name) {
                diagnostics.push(Diagnostic::new(
                    format!(
                        "field `{}` is declared more than once in struct `{}`",
                        field.name, declaration.name
                    ),
                    field.span,
                ));
            }
        }
    }
    validate_declared_types(program, &structs, &mut diagnostics);
    reject_recursive_structs(&structs, &mut diagnostics);

    for constant in &program.constants {
        if builtins::predefined_constant(&constant.name).is_some() {
            diagnostics.push(Diagnostic::new(
                format!(
                    "constant `{}` conflicts with a predefined key constant",
                    constant.name
                ),
                constant.span,
            ));
            continue;
        }
        if constants
            .insert(constant.name.clone(), constant.ty.clone())
            .is_some()
        {
            diagnostics.push(Diagnostic::new(
                format!("constant `{}` is declared more than once", constant.name),
                constant.span,
            ));
        }
        if let Some(existing) = top_level_names.insert(constant.name.clone(), "constant") {
            diagnostics.push(Diagnostic::new(
                format!(
                    "constant `{}` conflicts with an existing {existing}",
                    constant.name
                ),
                constant.span,
            ));
        }
    }

    for global in &program.globals {
        if globals
            .insert(global.name.clone(), global.ty.clone())
            .is_some()
        {
            diagnostics.push(Diagnostic::new(
                format!("global `{}` is declared more than once", global.name),
                global.span,
            ));
        }
        if let Some(existing) = top_level_names.insert(global.name.clone(), "global variable") {
            diagnostics.push(Diagnostic::new(
                format!(
                    "global `{}` conflicts with an existing {existing}",
                    global.name
                ),
                global.span,
            ));
        }
    }

    let mut invalid_constants = program
        .constants
        .iter()
        .filter_map(|constant| {
            if validate_compile_time_structure(
                &constant.init,
                &globals,
                "constant expressions",
                &mut diagnostics,
            ) {
                None
            } else {
                Some(constant.name.clone())
            }
        })
        .collect::<HashSet<_>>();
    invalid_constants.extend(invalid_length_constants);
    invalid_constants.extend(
        program
            .constants
            .iter()
            .filter(|constant| type_resolution_failed(&constant.ty, &structs))
            .map(|constant| constant.name.clone()),
    );
    let invalid_globals = program
        .globals
        .iter()
        .enumerate()
        .filter_map(|(index, global)| {
            (type_resolution_failed(&global.ty, &structs)
                || !validate_compile_time_structure(
                    &global.init,
                    &globals,
                    "global initializers",
                    &mut diagnostics,
                ))
            .then_some(index)
        })
        .collect::<HashSet<_>>();

    let mut functions = builtins();
    let mut entry_counts: HashMap<FunctionKind, usize> = HashMap::new();
    for function in &program.functions {
        if function.kind == FunctionKind::Named {
            let signature = Signature {
                params: function
                    .params
                    .iter()
                    .map(|param| param.ty.clone())
                    .collect(),
                return_type: function.return_type.clone(),
            };
            if functions.insert(function.name.clone(), signature).is_some() {
                diagnostics.push(Diagnostic::new(
                    format!(
                        "function `{}` conflicts with an existing function",
                        function.name
                    ),
                    function.span,
                ));
            }
            if let Some(existing) = top_level_names.insert(function.name.clone(), "function")
                && existing != "function"
            {
                diagnostics.push(Diagnostic::new(
                    format!(
                        "function `{}` conflicts with an existing {existing}",
                        function.name
                    ),
                    function.span,
                ));
            }
        } else {
            *entry_counts.entry(function.kind).or_default() += 1;
        }
    }

    for (kind, name) in [
        (FunctionKind::Start, "start"),
        (FunctionKind::Update, "update"),
        (FunctionKind::Draw, "draw"),
    ] {
        match entry_counts.get(&kind).copied().unwrap_or(0) {
            0 => diagnostics.push(Diagnostic::new(
                format!("game is missing its `{name}` entry point"),
                program.title_span,
            )),
            1 => {}
            _ => diagnostics.push(Diagnostic::new(
                format!("game declares `{name}` more than once"),
                program.title_span,
            )),
        }
    }

    let predefined_values = builtins::CONSTANTS
        .iter()
        .map(|constant| (constant.name.to_owned(), constant.value.clone()))
        .collect::<HashMap<_, _>>();
    for constant in &program.constants {
        if invalid_constants.contains(&constant.name)
            || type_resolution_failed(&constant.ty, &structs)
        {
            continue;
        }
        let mut checker = FunctionChecker::new(
            &globals,
            &constants,
            &predefined_values,
            &structs,
            &functions,
            ReturnType::Void,
            &mut diagnostics,
        );
        if let Some(actual) =
            checker.require_value_as(&constant.init, &constant.ty, "constant initializer")
            && actual != constant.ty
        {
            checker.error(
                format!(
                    "constant `{}` has type `{}`, but its initializer has type `{}`",
                    constant.name,
                    constant.ty.name(),
                    actual.name()
                ),
                constant.init.span,
            );
        }
    }

    for (index, global) in program.globals.iter().enumerate() {
        if invalid_globals.contains(&index) {
            continue;
        }
        let mut checker = FunctionChecker::new(
            &globals,
            &constants,
            &predefined_values,
            &structs,
            &functions,
            ReturnType::Void,
            &mut diagnostics,
        );
        if let Some(actual) =
            checker.require_value_as(&global.init, &global.ty, "global initializer")
            && actual != global.ty
        {
            checker.error(
                format!(
                    "global `{}` has type `{}`, but its initializer has type `{}`",
                    global.name,
                    global.ty.name(),
                    actual.name()
                ),
                global.init.span,
            );
        }
    }

    let constant_defs = program
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
        .collect();
    let mut evaluator =
        ConstantEvaluator::new(constant_defs, &globals, &structs, &invalid_constants);
    let constant_names = program
        .constants
        .iter()
        .map(|constant| constant.name.clone())
        .collect::<Vec<_>>();
    for name in constant_names {
        if invalid_constants.contains(&name) || evaluator.failed(&name) {
            continue;
        }
        if let Err(error) = evaluator.evaluate_named(&name) {
            diagnostics.push(error);
        }
    }
    let evaluated_constants = evaluator.values.clone();
    for constant in &mut program.constants {
        constant.value = evaluated_constants.get(&constant.name).cloned();
    }

    for (index, global) in program.globals.iter_mut().enumerate() {
        if invalid_globals.contains(&index) {
            continue;
        }
        match evaluate_initializer(
            &global.init,
            &global.ty,
            &evaluated_constants,
            &globals,
            &structs,
        ) {
            Ok(value) if value.ty() == global.ty => global.value = Some(value),
            Ok(_) => {}
            Err(message) => diagnostics.push(Diagnostic::new(message, global.init.span)),
        }
    }

    for function in &program.functions {
        if function.kind == FunctionKind::Update
            && (function.params.len() != 1 || function.params[0].ty != ValueType::F32)
        {
            diagnostics.push(Diagnostic::new(
                "`update` must take exactly one `f32` frame-delta parameter",
                function.span,
            ));
        }
        check_function(
            function,
            &globals,
            &constants,
            &evaluated_constants,
            &structs,
            &functions,
            &mut diagnostics,
        );
    }

    deduplicate_diagnostics(&mut diagnostics);
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

#[derive(Clone)]
struct LengthConstantDefinition {
    ty: ValueType,
    init: Expr,
    span: Span,
}

struct LengthConstantEvaluator {
    definitions: HashMap<String, LengthConstantDefinition>,
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
                        LengthConstantDefinition {
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
        definition: &LengthConstantDefinition,
    ) -> Result<ConstantValue, Diagnostic> {
        let mut ty = definition.ty.clone();
        let mut diagnostics = Vec::new();
        resolve_value_type(&mut ty, self, &mut diagnostics);
        let structs = self.resolve_structs_for_type(&ty, &mut diagnostics);
        if let Some(diagnostic) = diagnostics.into_iter().next() {
            return Err(diagnostic);
        }
        let value = evaluate_typed_expression(&definition.init, &ty, &structs, |dependency| {
            self.evaluate_value(dependency, definition.init.span)
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

fn resolve_program_types(
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

fn push_unique_diagnostic(diagnostics: &mut Vec<Diagnostic>, diagnostic: Diagnostic) {
    if !diagnostics.contains(&diagnostic) {
        diagnostics.push(diagnostic);
    }
}

fn deduplicate_diagnostics(diagnostics: &mut Vec<Diagnostic>) {
    let mut seen = HashSet::new();
    diagnostics.retain(|diagnostic| seen.insert((diagnostic.message.clone(), diagnostic.span)));
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

fn type_resolution_failed(ty: &ValueType, structs: &HashMap<String, StructDecl>) -> bool {
    fn visit(
        ty: &ValueType,
        structs: &HashMap<String, StructDecl>,
        visiting: &mut HashSet<String>,
    ) -> bool {
        match ty {
            ValueType::Array { element, length } => {
                matches!(length, ArrayLength::Invalid) || visit(element, structs, visiting)
            }
            ValueType::Struct(name) => {
                if !visiting.insert(name.clone()) {
                    return false;
                }
                let failed = structs.get(name).is_some_and(|declaration| {
                    declaration
                        .fields
                        .iter()
                        .any(|field| visit(&field.ty, structs, visiting))
                });
                visiting.remove(name);
                failed
            }
            ValueType::I32 | ValueType::F32 | ValueType::Bool => false,
        }
    }

    visit(ty, structs, &mut HashSet::new())
}

fn validate_declared_types(
    program: &Program,
    structs: &HashMap<String, StructDecl>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for declaration in &program.structs {
        for field in &declaration.fields {
            validate_known_type(&field.ty, field.span, structs, diagnostics);
        }
    }
    for constant in &program.constants {
        validate_known_type(&constant.ty, constant.span, structs, diagnostics);
    }
    for global in &program.globals {
        validate_known_type(&global.ty, global.span, structs, diagnostics);
    }
    for function in &program.functions {
        for param in &function.params {
            validate_known_type(&param.ty, param.span, structs, diagnostics);
        }
        if let ReturnType::Value(ty) = &function.return_type {
            validate_known_type(ty, function.span, structs, diagnostics);
        }
        validate_block_types(&function.body, structs, diagnostics);
    }
}

fn validate_block_types(
    block: &Block,
    structs: &HashMap<String, StructDecl>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for statement in block {
        match &statement.kind {
            StmtKind::Let { ty, .. } => {
                validate_known_type(ty, statement.span, structs, diagnostics);
            }
            StmtKind::If {
                then_block,
                else_block,
                ..
            } => {
                validate_block_types(then_block, structs, diagnostics);
                if let Some(else_block) = else_block {
                    validate_block_types(else_block, structs, diagnostics);
                }
            }
            StmtKind::While { body, .. } => {
                validate_block_types(body, structs, diagnostics);
            }
            StmtKind::For { body, .. } => {
                validate_block_types(body, structs, diagnostics);
            }
            StmtKind::Assign { .. } | StmtKind::Expr(_) | StmtKind::Return(_) => {}
        }
    }
}

fn validate_known_type(
    ty: &ValueType,
    span: Span,
    structs: &HashMap<String, StructDecl>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    match ty {
        ValueType::Struct(name) if !structs.contains_key(name) => diagnostics.push(
            Diagnostic::new(format!("unknown struct type `{name}`"), span),
        ),
        ValueType::Array { element, .. } => {
            validate_known_type(element, span, structs, diagnostics);
        }
        ValueType::I32 | ValueType::F32 | ValueType::Bool | ValueType::Struct(_) => {}
    }
}

fn reject_recursive_structs(
    structs: &HashMap<String, StructDecl>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for declaration in structs.values() {
        let mut visited = HashSet::new();
        if struct_reaches(&declaration.name, &declaration.name, structs, &mut visited) {
            diagnostics.push(Diagnostic::new(
                format!(
                    "recursive value type is not supported: struct `{}` contains itself",
                    declaration.name
                ),
                declaration.span,
            ));
        }
    }
}

fn struct_reaches(
    target: &str,
    current: &str,
    structs: &HashMap<String, StructDecl>,
    visited: &mut HashSet<String>,
) -> bool {
    if !visited.insert(current.to_owned()) {
        return false;
    }
    let Some(declaration) = structs.get(current) else {
        return false;
    };
    declaration.fields.iter().any(|field| {
        named_types(&field.ty).into_iter().any(|name| {
            name == target || struct_reaches(target, name, structs, &mut visited.clone())
        })
    })
}

fn named_types(ty: &ValueType) -> Vec<&str> {
    match ty {
        ValueType::Struct(name) => vec![name],
        ValueType::Array { element, .. } => named_types(element),
        ValueType::I32 | ValueType::F32 | ValueType::Bool => Vec::new(),
    }
}

fn validate_compile_time_structure(
    expression: &Expr,
    globals: &HashMap<String, ValueType>,
    description: &str,
    diagnostics: &mut Vec<Diagnostic>,
) -> bool {
    let mut valid = true;
    match &expression.kind {
        ExprKind::Variable(name) if globals.contains_key(name) => {
            diagnostics.push(Diagnostic::new(
                format!("{description} cannot reference mutable global `{name}`"),
                expression.span,
            ));
            valid = false;
        }
        ExprKind::Unary { operand, .. } => {
            valid &= validate_compile_time_structure(operand, globals, description, diagnostics);
        }
        ExprKind::Binary { left, right, .. } => {
            valid &= validate_compile_time_structure(left, globals, description, diagnostics);
            valid &= validate_compile_time_structure(right, globals, description, diagnostics);
        }
        ExprKind::Call { name, args } => {
            diagnostics.push(Diagnostic::new(
                format!("{description} cannot call function `{name}`"),
                expression.span,
            ));
            valid = false;
            for argument in args {
                valid &=
                    validate_compile_time_structure(argument, globals, description, diagnostics);
            }
        }
        ExprKind::Conversion { args, .. } => {
            for argument in args {
                valid &=
                    validate_compile_time_structure(argument, globals, description, diagnostics);
            }
        }
        ExprKind::ArrayLiteral(elements) => {
            for element in elements {
                valid &=
                    validate_compile_time_structure(element, globals, description, diagnostics);
            }
        }
        ExprKind::Index { base, index } => {
            valid &= validate_compile_time_structure(base, globals, description, diagnostics);
            valid &= validate_compile_time_structure(index, globals, description, diagnostics);
        }
        ExprKind::StructLiteral { fields, .. } => {
            for field in fields {
                valid &= validate_compile_time_structure(
                    &field.value,
                    globals,
                    description,
                    diagnostics,
                );
            }
        }
        ExprKind::Field { base, .. } => {
            valid &= validate_compile_time_structure(base, globals, description, diagnostics);
        }
        ExprKind::I32(_) | ExprKind::F32(_) | ExprKind::Bool(_) | ExprKind::Variable(_) => {}
    }
    valid
}

fn builtins() -> HashMap<String, Signature> {
    builtins::FUNCTIONS
        .iter()
        .map(|builtin| {
            (
                builtin.name.to_owned(),
                Signature {
                    params: builtin.params.to_vec(),
                    return_type: builtin.return_type.clone(),
                },
            )
        })
        .collect()
}

fn check_function(
    function: &Function,
    globals: &HashMap<String, ValueType>,
    constants: &HashMap<String, ValueType>,
    constant_values: &HashMap<String, ConstantValue>,
    structs: &HashMap<String, StructDecl>,
    functions: &HashMap<String, Signature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    for param in &function.params {
        if matches!(param.ty, ValueType::Array { .. }) {
            diagnostics.push(Diagnostic::new(
                "arrays are not supported as function parameters yet",
                param.span,
            ));
        }
    }
    if matches!(
        function.return_type,
        ReturnType::Value(ValueType::Array { .. })
    ) {
        diagnostics.push(Diagnostic::new(
            "arrays are not supported as function return types yet",
            function.span,
        ));
    }
    let mut checker = FunctionChecker::new(
        globals,
        constants,
        constant_values,
        structs,
        functions,
        function.return_type.clone(),
        diagnostics,
    );
    for param in &function.params {
        if builtins::predefined_constant(&param.name).is_some() {
            checker.error(
                format!(
                    "parameter `{}` conflicts with a predefined key constant",
                    param.name
                ),
                param.span,
            );
        }
        if checker.scopes[0]
            .insert(
                param.name.clone(),
                Binding {
                    ty: param.ty.clone(),
                    mutability: Mutability::Mutable,
                },
            )
            .is_some()
        {
            checker.error(
                format!("parameter `{}` is declared more than once", param.name),
                param.span,
            );
        }
    }
    checker.check_statements(&function.body);
    if function.return_type.value_type().is_some() && !block_returns(&function.body) {
        checker.error(
            format!(
                "function `{}` may finish without returning `{}`",
                function.name,
                function.return_type.name()
            ),
            function.span,
        );
    }
}

struct FunctionChecker<'a> {
    globals: &'a HashMap<String, ValueType>,
    constants: &'a HashMap<String, ValueType>,
    constant_values: &'a HashMap<String, ConstantValue>,
    structs: &'a HashMap<String, StructDecl>,
    functions: &'a HashMap<String, Signature>,
    return_type: ReturnType,
    scopes: Vec<HashMap<String, Binding>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

#[derive(Clone)]
struct Lvalue {
    ty: ValueType,
    mutability: Mutability,
    root_name: String,
}

impl<'a> FunctionChecker<'a> {
    fn new(
        globals: &'a HashMap<String, ValueType>,
        constants: &'a HashMap<String, ValueType>,
        constant_values: &'a HashMap<String, ConstantValue>,
        structs: &'a HashMap<String, StructDecl>,
        functions: &'a HashMap<String, Signature>,
        return_type: ReturnType,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        Self {
            globals,
            constants,
            constant_values,
            structs,
            functions,
            return_type,
            scopes: vec![HashMap::new()],
            diagnostics,
        }
    }

    fn check_statements(&mut self, statements: &Block) {
        for statement in statements {
            self.check_statement(statement);
        }
    }

    fn check_nested_block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        self.check_statements(block);
        self.scopes.pop();
    }

    fn check_statement(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Let { name, ty, init } => {
                if builtins::predefined_constant(name).is_some() {
                    self.error(
                        format!("variable `{name}` conflicts with a predefined key constant"),
                        statement.span,
                    );
                }
                if let Some(actual) = self.require_value_as(init, ty, "variable initializer")
                    && actual != *ty
                {
                    self.error(
                        format!(
                            "variable `{name}` has type `{}`, but its initializer has type `{}`",
                            ty.name(),
                            actual.name()
                        ),
                        init.span,
                    );
                }
                let scope = self
                    .scopes
                    .last_mut()
                    .expect("a function always has a scope");
                if scope
                    .insert(
                        name.clone(),
                        Binding {
                            ty: ty.clone(),
                            mutability: Mutability::Mutable,
                        },
                    )
                    .is_some()
                {
                    self.error(
                        format!("variable `{name}` is declared more than once in this block"),
                        statement.span,
                    );
                }
            }
            StmtKind::Assign { target, op, value } => {
                let Some(lvalue) = self.infer_lvalue(target) else {
                    return;
                };
                let actual = if *op == AssignOp::Set {
                    self.require_value_as(value, &lvalue.ty, "assignment")
                } else {
                    self.require_value(value, "compound assignment")
                };
                if lvalue.mutability == Mutability::LoopVariable {
                    self.error(
                        format!("loop variable `{}` is read-only", lvalue.root_name),
                        statement.span,
                    );
                    return;
                }
                if lvalue.mutability == Mutability::Constant {
                    let direct_variable = matches!(target.kind, ExprKind::Variable(_));
                    let message = if *op == AssignOp::Set && direct_variable {
                        format!("cannot assign to constant `{}`", lvalue.root_name)
                    } else if *op == AssignOp::Set {
                        format!("cannot assign through constant `{}`", lvalue.root_name)
                    } else if direct_variable {
                        format!(
                            "cannot use compound assignment on constant `{}`",
                            lvalue.root_name
                        )
                    } else {
                        format!(
                            "cannot use compound assignment through constant `{}`",
                            lvalue.root_name
                        )
                    };
                    self.error(message, statement.span);
                    return;
                }
                if *op != AssignOp::Set && !lvalue.ty.is_numeric() {
                    self.error(
                        format!(
                            "compound assignment requires a numeric target, but found `{}`",
                            lvalue.ty.name()
                        ),
                        statement.span,
                    );
                }
                if let Some(actual) = actual
                    && lvalue.ty != actual
                {
                    let direct_variable = matches!(target.kind, ExprKind::Variable(_));
                    let message = if *op == AssignOp::Set && direct_variable {
                        format!(
                            "cannot assign `{}` to variable `{}` of type `{}`",
                            actual.name(),
                            lvalue.root_name,
                            lvalue.ty.name()
                        )
                    } else if *op == AssignOp::Set {
                        format!(
                            "cannot assign `{}` to target of type `{}`",
                            actual.name(),
                            lvalue.ty.name()
                        )
                    } else if direct_variable {
                        format!(
                            "compound assignment to `{}` expects `{}`, but found `{}`",
                            lvalue.root_name,
                            lvalue.ty.name(),
                            actual.name()
                        )
                    } else {
                        format!(
                            "compound assignment expects `{}`, but found `{}`",
                            lvalue.ty.name(),
                            actual.name()
                        )
                    };
                    self.error(message, value.span);
                }
            }
            StmtKind::Expr(expression) => {
                self.infer_expr(expression);
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => {
                self.require_bool(condition, "`if` condition");
                self.check_nested_block(then_block);
                if let Some(else_block) = else_block {
                    self.check_nested_block(else_block);
                }
            }
            StmtKind::While { condition, body } => {
                self.require_bool(condition, "`while` condition");
                self.check_nested_block(body);
            }
            StmtKind::For {
                name,
                name_span,
                lower,
                upper,
                body,
            } => {
                for (bound, description) in
                    [(lower, "range lower bound"), (upper, "range upper bound")]
                {
                    if let Some(actual) = self.require_value(bound, description)
                        && actual != ValueType::I32
                    {
                        self.error(
                            format!("{description} must be `i32`, found `{}`", actual.name()),
                            bound.span,
                        );
                    }
                }
                if builtins::predefined_constant(name).is_some() {
                    self.error(
                        format!("loop variable `{name}` conflicts with a predefined key constant"),
                        *name_span,
                    );
                }
                self.scopes.push(HashMap::from([(
                    name.clone(),
                    Binding {
                        ty: ValueType::I32,
                        mutability: Mutability::LoopVariable,
                    },
                )]));
                self.check_statements(body);
                self.scopes.pop();
            }
            StmtKind::Return(value) => match (self.return_type.clone(), value) {
                (ReturnType::Void, None) => {}
                (ReturnType::Void, Some(value)) => {
                    self.infer_expr(value);
                    self.error("void function cannot return a value", value.span);
                }
                (ReturnType::Value(expected), None) => self.error(
                    format!("return requires a value of type `{}`", expected.name()),
                    statement.span,
                ),
                (ReturnType::Value(expected), Some(value)) => {
                    match if matches!(value.kind, ExprKind::ArrayLiteral(_)) {
                        self.require_value_as(value, &expected, "return")
                            .map(ReturnType::Value)
                    } else {
                        self.infer_expr(value)
                    } {
                        Some(ReturnType::Value(actual)) if actual != expected => self.error(
                            format!(
                                "return expects `{}`, but found `{}`",
                                expected.name(),
                                actual.name()
                            ),
                            value.span,
                        ),
                        Some(ReturnType::Void) => self.error(
                            format!("return expects `{}`, but found `void`", expected.name()),
                            value.span,
                        ),
                        _ => {}
                    }
                }
            },
        }
    }

    fn infer_expr(&mut self, expression: &Expr) -> Option<ReturnType> {
        match &expression.kind {
            ExprKind::I32(value) => {
                if i32::try_from(*value).is_err() {
                    self.error("integer literal does not fit in `i32`", expression.span);
                }
                Some(ValueType::I32.into())
            }
            ExprKind::F32(value) => {
                if !value.is_finite() {
                    self.error("floating-point literal must be finite", expression.span);
                }
                Some(ValueType::F32.into())
            }
            ExprKind::Bool(_) => Some(ValueType::Bool.into()),
            ExprKind::ArrayLiteral(_) => {
                self.error(
                    "array literal requires an explicit array type annotation",
                    expression.span,
                );
                None
            }
            ExprKind::StructLiteral { name, fields } => self
                .infer_struct_literal(name, fields, expression.span)
                .map(ReturnType::Value),
            ExprKind::Variable(name) => match self.binding(name) {
                Some(binding) => Some(binding.ty.into()),
                None => {
                    self.error(format!("unknown variable `{name}`"), expression.span);
                    None
                }
            },
            ExprKind::Index { base, index } => self
                .infer_index(base, index, expression.span)
                .map(ReturnType::Value),
            ExprKind::Field {
                base,
                name,
                name_span,
            } => self
                .infer_field(base, name, *name_span)
                .map(ReturnType::Value),
            ExprKind::Unary { op, operand } => {
                if *op == UnaryOp::Negate
                    && matches!(operand.kind, ExprKind::I32(value) if value == 2_147_483_648)
                {
                    return Some(ValueType::I32.into());
                }
                let ty = self.require_value(operand, "unary operator")?;
                match (op, ty.clone()) {
                    (UnaryOp::Negate, ValueType::I32 | ValueType::F32) => Some(ty.into()),
                    (UnaryOp::Not, ValueType::Bool) => Some(ValueType::Bool.into()),
                    (UnaryOp::Negate, _) => {
                        self.error("unary `-` requires `i32` or `f32`", expression.span);
                        None
                    }
                    (UnaryOp::Not, _) => {
                        self.error("unary `!` requires `bool`", expression.span);
                        None
                    }
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left_type = self.require_value(left, "operator operand");
                let right_type = self.require_value(right, "operator operand");
                let (Some(left_type), Some(right_type)) = (left_type, right_type) else {
                    return None;
                };
                if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    if left_type != ValueType::Bool || right_type != ValueType::Bool {
                        self.error(
                            format!(
                                "Boolean operator requires `bool` operands, found `{}` and `{}`",
                                left_type.name(),
                                right_type.name()
                            ),
                            expression.span,
                        );
                        return None;
                    }
                    return Some(ValueType::Bool.into());
                }
                if left_type != right_type {
                    self.error(
                        format!(
                            "operator operands must have the same type, found `{}` and `{}`",
                            left_type.name(),
                            right_type.name()
                        ),
                        expression.span,
                    );
                    return None;
                }
                match op {
                    BinaryOp::Add | BinaryOp::Subtract | BinaryOp::Multiply | BinaryOp::Divide => {
                        if left_type.is_numeric() {
                            Some(left_type.into())
                        } else {
                            self.error(
                                "arithmetic requires `i32` or `f32` operands",
                                expression.span,
                            );
                            None
                        }
                    }
                    BinaryOp::Less
                    | BinaryOp::LessEqual
                    | BinaryOp::Greater
                    | BinaryOp::GreaterEqual => {
                        if left_type.is_numeric() {
                            Some(ValueType::Bool.into())
                        } else {
                            self.error(
                                "ordering comparison requires numeric operands",
                                expression.span,
                            );
                            None
                        }
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if matches!(left_type, ValueType::I32 | ValueType::F32 | ValueType::Bool) {
                            Some(ValueType::Bool.into())
                        } else {
                            self.error(
                                "equality comparison requires scalar operands",
                                expression.span,
                            );
                            None
                        }
                    }
                    BinaryOp::LogicalAnd | BinaryOp::LogicalOr => unreachable!(),
                }
            }
            ExprKind::Call { name, args } => {
                let Some(signature) = self.functions.get(name).cloned() else {
                    self.error(format!("unknown function `{name}`"), expression.span);
                    for arg in args {
                        self.infer_expr(arg);
                    }
                    return None;
                };
                if args.len() != signature.params.len() {
                    self.error(
                        format!(
                            "function `{name}` expects {} argument(s), but received {}",
                            signature.params.len(),
                            args.len()
                        ),
                        expression.span,
                    );
                }
                for (index, arg) in args.iter().enumerate() {
                    let actual = self.require_value(arg, "function argument");
                    if let (Some(expected), Some(actual)) =
                        (signature.params.get(index).cloned(), actual)
                        && expected != actual
                    {
                        self.error(
                            format!(
                                "argument {} to `{name}` expects `{}`, but found `{}`",
                                index + 1,
                                expected.name(),
                                actual.name()
                            ),
                            arg.span,
                        );
                    }
                }
                Some(signature.return_type)
            }
            ExprKind::Conversion { target, args } => {
                if args.len() != 1 {
                    self.error(
                        format!(
                            "conversion to `{}` expects 1 argument, but received {}",
                            target.name(),
                            args.len()
                        ),
                        expression.span,
                    );
                    for arg in args {
                        self.infer_expr(arg);
                    }
                    return None;
                }
                match self.infer_expr(&args[0]) {
                    Some(ReturnType::Value(ValueType::I32 | ValueType::F32)) => {
                        Some(target.clone().into())
                    }
                    Some(ReturnType::Value(ValueType::Bool)) => {
                        self.error("cannot convert `bool` to a numeric type", args[0].span);
                        None
                    }
                    Some(ReturnType::Value(ValueType::Array { .. })) => {
                        self.error("cannot convert an array to a numeric type", args[0].span);
                        None
                    }
                    Some(ReturnType::Value(ValueType::Struct(name))) => {
                        self.error(
                            format!("cannot convert struct `{name}` to a numeric type"),
                            args[0].span,
                        );
                        None
                    }
                    Some(ReturnType::Void) => {
                        self.error("cannot convert `void` to a numeric type", args[0].span);
                        None
                    }
                    None => None,
                }
            }
        }
    }

    fn require_value_as(
        &mut self,
        expression: &Expr,
        expected: &ValueType,
        description: &str,
    ) -> Option<ValueType> {
        if expected.has_invalid_array_length() {
            return Some(expected.clone());
        }
        if let ExprKind::ArrayLiteral(elements) = &expression.kind {
            let Some((element_type, length)) = expected.resolved_array() else {
                self.error(
                    format!(
                        "{description} is an array literal, but the declared type is `{}`",
                        expected.name()
                    ),
                    expression.span,
                );
                return None;
            };
            if elements.len() != length {
                self.error(
                    format!(
                        "expected array length {length}, found {} elements",
                        elements.len()
                    ),
                    expression.span,
                );
            }
            for element in elements {
                if let Some(actual) = self.require_value_as(element, element_type, "array element")
                    && actual != *element_type
                {
                    self.error(
                        format!(
                            "array element expects `{}`, but found `{}`",
                            element_type.name(),
                            actual.name()
                        ),
                        element.span,
                    );
                }
            }
            return Some(expected.clone());
        }
        self.require_value(expression, description)
    }

    fn infer_index(&mut self, base: &Expr, index: &Expr, span: Span) -> Option<ValueType> {
        let base_type = self.require_value(base, "indexed expression")?;
        let index_type = self.require_value(index, "array index");
        if let Some(index_type) = index_type
            && index_type != ValueType::I32
        {
            self.error(
                format!("array index must be i32, found `{}`", index_type.name()),
                index.span,
            );
        }
        if base_type.has_invalid_array_length() {
            return None;
        }
        let Some((element, length)) = base_type.resolved_array() else {
            self.error(
                format!("cannot index value of type `{}`", base_type.name()),
                span,
            );
            return None;
        };
        self.check_constant_index(index, length);
        Some(element.clone())
    }

    fn infer_struct_literal(
        &mut self,
        name: &str,
        fields: &[crate::ast::FieldInitializer],
        span: Span,
    ) -> Option<ValueType> {
        let Some(declaration) = self.structs.get(name).cloned() else {
            self.error(format!("unknown struct type `{name}`"), span);
            for field in fields {
                self.infer_expr(&field.value);
            }
            return None;
        };
        let mut seen = HashSet::new();
        for initializer in fields {
            if !seen.insert(initializer.name.clone()) {
                self.error(
                    format!(
                        "field `{}` is initialized more than once in `{name}`",
                        initializer.name
                    ),
                    initializer.span,
                );
                self.infer_expr(&initializer.value);
                continue;
            }
            let Some(field) = declaration
                .fields
                .iter()
                .find(|field| field.name == initializer.name)
            else {
                self.error(
                    format!("type `{name}` has no field named `{}`", initializer.name),
                    initializer.span,
                );
                self.infer_expr(&initializer.value);
                continue;
            };
            if let Some(actual) =
                self.require_value_as(&initializer.value, &field.ty, "field initializer")
                && actual != field.ty
            {
                self.error(
                    format!(
                        "field `{}` of `{name}` expects `{}`, but found `{}`",
                        field.name,
                        field.ty.name(),
                        actual.name()
                    ),
                    initializer.value.span,
                );
            }
        }
        for field in &declaration.fields {
            if !seen.contains(&field.name) {
                self.error(
                    format!("missing initializer for field `{}` of `{name}`", field.name),
                    span,
                );
            }
        }
        Some(ValueType::Struct(name.to_owned()))
    }

    fn infer_field(&mut self, base: &Expr, name: &str, name_span: Span) -> Option<ValueType> {
        let base_type = self.require_value(base, "field access")?;
        let ValueType::Struct(struct_name) = base_type else {
            self.error(
                format!("type `{}` has no fields", base_type.name()),
                name_span,
            );
            return None;
        };
        let Some(declaration) = self.structs.get(&struct_name) else {
            self.error(format!("unknown struct type `{struct_name}`"), base.span);
            return None;
        };
        let Some(field) = declaration.fields.iter().find(|field| field.name == name) else {
            self.error(
                format!("type `{struct_name}` has no field named `{name}`"),
                name_span,
            );
            return None;
        };
        Some(field.ty.clone())
    }

    fn infer_lvalue(&mut self, expression: &Expr) -> Option<Lvalue> {
        match &expression.kind {
            ExprKind::Variable(name) => match self.binding(name) {
                Some(binding) => Some(Lvalue {
                    ty: binding.ty,
                    mutability: binding.mutability,
                    root_name: name.clone(),
                }),
                None => {
                    self.error(
                        format!("cannot assign to unknown variable `{name}`"),
                        expression.span,
                    );
                    None
                }
            },
            ExprKind::Index { base, index } => {
                let root = self.infer_lvalue(base)?;
                let index_type = self.require_value(index, "array index");
                if let Some(index_type) = index_type
                    && index_type != ValueType::I32
                {
                    self.error(
                        format!("array index must be i32, found `{}`", index_type.name()),
                        index.span,
                    );
                }
                if root.ty.has_invalid_array_length() {
                    return None;
                }
                let Some((element, length)) = root.ty.resolved_array() else {
                    self.error(
                        format!("cannot index value of type `{}`", root.ty.name()),
                        expression.span,
                    );
                    return None;
                };
                let element = element.clone();
                self.check_constant_index(index, length);
                Some(Lvalue {
                    ty: element,
                    mutability: root.mutability,
                    root_name: root.root_name,
                })
            }
            ExprKind::Field {
                base,
                name,
                name_span,
            } => {
                let root = self.infer_lvalue(base)?;
                let ValueType::Struct(struct_name) = &root.ty else {
                    self.error(
                        format!("type `{}` has no fields", root.ty.name()),
                        *name_span,
                    );
                    return None;
                };
                let Some(declaration) = self.structs.get(struct_name) else {
                    self.error(format!("unknown struct type `{struct_name}`"), base.span);
                    return None;
                };
                let Some(field) = declaration
                    .fields
                    .iter()
                    .find(|field| field.name == name.as_str())
                else {
                    self.error(
                        format!("type `{struct_name}` has no field named `{name}`"),
                        *name_span,
                    );
                    return None;
                };
                Some(Lvalue {
                    ty: field.ty.clone(),
                    mutability: root.mutability,
                    root_name: root.root_name,
                })
            }
            _ => {
                self.infer_expr(expression);
                self.error("invalid assignment target", expression.span);
                None
            }
        }
    }

    fn check_constant_index(&mut self, index: &Expr, length: usize) {
        let result = evaluate_expression(index, |name| {
            self.constant_values.get(name).cloned().ok_or_else(|| {
                Diagnostic::new(
                    format!("`{name}` is not a compile-time constant"),
                    index.span,
                )
            })
        });
        let Ok(ConstantValue::I32(value)) = result else {
            return;
        };
        if value < 0 || usize::try_from(value).map_or(true, |value| value >= length) {
            self.error(
                format!("constant index {value} is out of bounds for length {length}"),
                index.span,
            );
        }
    }

    fn require_value(&mut self, expression: &Expr, description: &str) -> Option<ValueType> {
        match self.infer_expr(expression) {
            Some(ReturnType::Value(ty)) => Some(ty),
            Some(ReturnType::Void) => {
                self.error(
                    format!("{description} requires a value, but found `void`"),
                    expression.span,
                );
                None
            }
            None => None,
        }
    }

    fn require_bool(&mut self, expression: &Expr, description: &str) {
        if let Some(actual) = self.require_value(expression, description)
            && actual != ValueType::Bool
        {
            self.error(
                format!("{description} must be `bool`, found `{}`", actual.name()),
                expression.span,
            );
        }
    }

    fn binding(&self, name: &str) -> Option<Binding> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .or_else(|| {
                self.globals.get(name).cloned().map(|ty| Binding {
                    ty,
                    mutability: Mutability::Mutable,
                })
            })
            .or_else(|| {
                self.constants.get(name).cloned().map(|ty| Binding {
                    ty,
                    mutability: Mutability::Constant,
                })
            })
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }
}

#[derive(Clone)]
struct ConstantDefinition {
    ty: ValueType,
    init: Expr,
    span: Span,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum VisitState {
    Visiting,
    Complete,
    Failed,
}

struct ConstantEvaluator<'a> {
    definitions: HashMap<String, ConstantDefinition>,
    globals: &'a HashMap<String, ValueType>,
    structs: &'a HashMap<String, StructDecl>,
    invalid: &'a HashSet<String>,
    states: HashMap<String, VisitState>,
    values: HashMap<String, ConstantValue>,
    stack: Vec<String>,
}

impl<'a> ConstantEvaluator<'a> {
    fn new(
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

    fn failed(&self, name: &str) -> bool {
        matches!(self.states.get(name), Some(VisitState::Failed))
    }

    fn evaluate_named(&mut self, name: &str) -> Result<ConstantValue, Diagnostic> {
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
        let result = self
            .evaluate_typed_expr(&definition.init, &definition.ty)
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

    fn evaluate_typed_expr(
        &mut self,
        expression: &Expr,
        expected: &ValueType,
    ) -> Result<ConstantValue, Diagnostic> {
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
            let declaration = self.structs.get(name).cloned().ok_or_else(|| {
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
                    self.evaluate_typed_expr(&initializer.value, &field.ty)?,
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
                values.push(self.evaluate_typed_expr(element, element_type)?);
            }
            return Ok(ConstantValue::Array {
                element_type: Box::new(element_type.clone()),
                elements: values,
            });
        }
        self.evaluate_expr(expression)
    }
}

fn evaluate_initializer(
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
    evaluate_typed_expression(expression, expected, structs, &mut lookup)
        .map_err(|diagnostic| diagnostic.message)
}

fn evaluate_typed_expression<F>(
    expression: &Expr,
    expected: &ValueType,
    structs: &HashMap<String, StructDecl>,
    mut constant: F,
) -> Result<ConstantValue, Diagnostic>
where
    F: FnMut(&str) -> Result<ConstantValue, Diagnostic>,
{
    fn evaluate<F>(
        expression: &Expr,
        expected: &ValueType,
        structs: &HashMap<String, StructDecl>,
        constant: &mut F,
    ) -> Result<ConstantValue, Diagnostic>
    where
        F: FnMut(&str) -> Result<ConstantValue, Diagnostic>,
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
                    evaluate(&initializer.value, &field.ty, structs, constant)?,
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
                values.push(evaluate(element, element_type, structs, constant)?);
            }
            return Ok(ConstantValue::Array {
                element_type: Box::new(element_type.clone()),
                elements: values,
            });
        }
        evaluate_expression(expression, constant)
    }

    evaluate(expression, expected, structs, &mut constant)
}

fn evaluate_expression<F>(expression: &Expr, mut constant: F) -> Result<ConstantValue, Diagnostic>
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
            BinaryOp::Equal => Ok(ConstantValue::Bool(left == right)),
            BinaryOp::NotEqual => Ok(ConstantValue::Bool(left != right)),
            BinaryOp::Less => Ok(ConstantValue::Bool(left < right)),
            BinaryOp::LessEqual => Ok(ConstantValue::Bool(left <= right)),
            BinaryOp::Greater => Ok(ConstantValue::Bool(left > right)),
            BinaryOp::GreaterEqual => Ok(ConstantValue::Bool(left >= right)),
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => Err("Boolean operator requires `bool`"),
        },
        (ConstantValue::F32(left), ConstantValue::F32(right)) => match op {
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

fn block_returns(block: &Block) -> bool {
    block.iter().any(|statement| match &statement.kind {
        StmtKind::Return(_) => true,
        StmtKind::If {
            then_block,
            else_block: Some(else_block),
            ..
        } => block_returns(then_block) && block_returns(else_block),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{lexer, parser};

    fn errors(source: &str) -> Vec<Diagnostic> {
        let tokens = lexer::lex(source).expect("lexing should pass");
        let mut program = parser::parse(tokens).expect("parsing should pass");
        check(&mut program).expect_err("semantic checking should fail")
    }

    fn checked(source: &str) -> Program {
        let tokens = lexer::lex(source).expect("lexing should pass");
        let mut program = parser::parse(tokens).expect("parsing should pass");
        check(&mut program).expect("semantic checking should pass");
        program
    }

    #[test]
    fn reports_unknown_variable_with_source_span() {
        let source = "game \"Bad\"\nstart { missing = 1 }\nupdate(dt: f32) {}\ndraw {}";
        let errors = errors(source);
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("unknown variable `missing`"))
        );
        let rendered = errors[0].render(std::path::Path::new("bad.spk"), source);
        assert!(rendered.contains("bad.spk:2:"));
    }

    #[test]
    fn rejects_mismatched_assignment_and_implicit_conversion() {
        let errors = errors(
            "game \"Bad\"\nlet n: i32 = 1\nlet x: f32 = 10\nstart { n = 1.0 }\nupdate(dt: f32) {}\ndraw {}",
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("cannot assign `f32`"))
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("global `x` has type `f32`, but its initializer has type `i32`")
        }));
    }

    #[test]
    fn requires_return_on_all_paths() {
        let errors = errors(
            "game \"Bad\"\nfn value(flag: bool) -> i32 { if flag { return 1 } }\nstart {}\nupdate(dt: f32) {}\ndraw {}",
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("may finish without returning"))
        );
    }

    #[test]
    fn rejects_non_f32_update_delta() {
        let errors = errors("game \"Bad\"\nstart {}\nupdate(dt: i32) {}\ndraw {}");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("must take exactly one `f32`"))
        );
    }

    #[test]
    fn type_checks_graphics_builtin_arguments() {
        let errors = errors(
            "game \"Bad\"\nstart {}\nupdate(dt: f32) {}\ndraw { clear_rgb(0, true, 0) fill_rect(1, 2, 3) }",
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("argument 2 to `clear_rgb` expects `i32`, but found `bool`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("function `fill_rect` expects 7 argument(s), but received 3")
        }));
    }

    #[test]
    fn evaluates_constants_out_of_order_and_short_circuits() {
        let program = checked(
            r#"game "Constants"
const AREA: i32 = WIDTH * HEIGHT
const SAFE: bool = false && (1 / 0 == 0)
const HEIGHT: i32 = 2
const WIDTH: i32 = 3
const LARGE: bool = AREA >= 6
const READY: bool = !false && LARGE
let initial: i32 = AREA
start {}
update(dt: f32) {}
draw {}
"#,
        );
        assert_eq!(program.constants[0].value, Some(ConstantValue::I32(6)));
        assert_eq!(program.constants[1].value, Some(ConstantValue::Bool(false)));
        assert_eq!(program.constants[4].value, Some(ConstantValue::Bool(true)));
        assert_eq!(program.constants[5].value, Some(ConstantValue::Bool(true)));
        assert_eq!(program.globals[0].value, Some(ConstantValue::I32(6)));
    }

    #[test]
    fn rejects_constant_cycles_and_mutable_dependencies() {
        let errors = errors(
            r#"game "Bad constants"
const A: i32 = B
const B: i32 = A
let mutable: i32 = 1
const C: i32 = mutable
start {}
update(dt: f32) {}
draw {}
"#,
        );
        assert!(errors.iter().any(|error| {
            error.message.contains("cyclic constant definition")
                && error.message.contains('A')
                && error.message.contains('B')
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("cannot reference mutable global `mutable`")
        }));
    }

    #[test]
    fn validates_conversions_and_void_values() {
        checked(
            r#"game "Conversions"
fn show(value: i32) -> void { print_i32(value) return }
let a: f32 = f32(10)
let b: i32 = i32(4.75)
start { show(i32(a)) }
update(dt: f32) {}
draw {}
"#,
        );
        let errors = errors(
            r#"game "Bad"
fn show() -> void {}
let wrong: i32 = i32(true)
start { let value: i32 = show() i32() f32(1, 2) }
update(dt: f32) {}
draw {}
"#,
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("cannot convert `bool`"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("requires a value, but found `void`"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("expects 1 argument"))
        );
    }

    #[test]
    fn validates_compound_assignment_and_constant_immutability() {
        checked(
            r#"game "Mutation"
let x: i32 = 1
let y: f32 = 2.0
start { x += 2 x -= 1 y *= 2.0 y /= 2.0 }
update(dt: f32) {}
draw {}
"#,
        );
        let errors = errors(
            r#"game "Bad"
const LIMIT: i32 = 2
let flag: bool = false
let x: i32 = 1
start { LIMIT += 1 flag += true x += 1.0 LIMIT = 3 }
update(dt: f32) {}
draw {}
"#,
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("compound assignment on constant `LIMIT`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("compound assignment requires a numeric target")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("compound assignment to `x` expects `i32`, but found `f32`")
        }));
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("cannot assign to constant `LIMIT`"))
        );
    }

    #[test]
    fn reports_constant_division_and_overflow() {
        let errors = errors(
            r#"game "Bad"
const ZERO: i32 = 1 / 0
const HUGE: i32 = 2147483647 + 1
start {}
update(dt: f32) {}
draw {}
"#,
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("division by zero"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("constant-expression overflow"))
        );
    }

    #[test]
    fn evaluates_constant_conversions_and_numeric_boundaries() {
        let program = checked(
            r#"game "Boundaries"
const MIN: i32 = -2147483648
const MAX: i32 = 2147483647
const TRUNCATED: i32 = i32(-3.9)
const CLAMPED: i32 = i32(2147483648.0)
const FLOATED: f32 = f32(MAX)
const SAME: f32 = f32(FLOATED)
start {}
update(dt: f32) {}
draw {}
"#,
        );
        assert_eq!(
            program.constants[0].value,
            Some(ConstantValue::I32(i32::MIN))
        );
        assert_eq!(
            program.constants[1].value,
            Some(ConstantValue::I32(i32::MAX))
        );
        assert_eq!(program.constants[2].value, Some(ConstantValue::I32(-3)));
        assert_eq!(
            program.constants[3].value,
            Some(ConstantValue::I32(i32::MAX))
        );
        assert_eq!(
            program.constants[4].value,
            Some(ConstantValue::F32(i32::MAX as f32))
        );
        assert_eq!(program.constants[5].value, program.constants[4].value);
    }

    #[test]
    fn validates_boolean_operands_and_return_forms() {
        let errors = errors(
            r#"game "Bad"
fn value() -> i32 { return }
fn effect() -> void { return 1 }
fn other() -> i32 { return effect() }
start { if 1 && true {} if effect() {} let bad: i32 = effect() }
update(dt: f32) {}
draw {}
"#,
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("Boolean operator requires `bool`"))
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("return requires a value of type `i32`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("void function cannot return a value")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("return expects `i32`, but found `void`")
        }));
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("requires a value, but found `void`"))
        );
    }

    #[test]
    fn reports_top_level_name_collisions_and_invalid_initializers() {
        let errors = errors(
            r#"game "Bad"
const BAD: i32 = runtime()
const SAME: i32 = 1
const SAME: i32 = 2
let SAME: i32 = 3
fn SAME() -> i32 { return 4 }
fn runtime() -> i32 { return 1 }
start {}
update(dt: f32) {}
draw {}
"#,
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("constant `SAME` is declared more than once")
        }));
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("global `SAME` conflicts"))
        );
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("function `SAME` conflicts"))
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("constant expressions cannot call function `runtime`")
        }));
    }

    #[test]
    fn rejects_disallowed_constant_dependencies_even_when_short_circuited() {
        let errors = errors(
            r#"game "Bad"
let mutable: i32 = 1
const CALL: bool = false && runtime()
const GLOBAL: bool = true || mutable == 1
fn runtime() -> bool { return true }
start {}
update(dt: f32) {}
draw {}
"#,
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("constant expressions cannot call function `runtime`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("constant expressions cannot reference mutable global `mutable`")
        }));
    }

    #[test]
    fn validates_input_builtins_and_predefined_key_constants() {
        checked(
            r#"game "Input"
const FIRST: i32 = KEY_W
let combined: i32 = KEY_A + KEY_S + KEY_D + KEY_UP + KEY_DOWN + KEY_LEFT + KEY_RIGHT + KEY_SPACE + KEY_ENTER + KEY_ESCAPE
start { quit() }
update(dt: f32) {
    if key_down(KEY_W) || key_pressed(KEY_SPACE) || key_released(KEY_ENTER) {}
}
draw {}
"#,
        );

        let errors = errors(
            r#"game "Bad input"
const KEY_W: i32 = 100
let KEY_A: i32 = 1
fn KEY_S() -> void {}
start {
    key_down(true)
    key_pressed()
    key_released(KEY_D, KEY_W)
    let value: i32 = quit()
}
update(dt: f32) {}
draw {}
"#,
        );
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("constant `KEY_W` conflicts with a predefined key constant")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("global `KEY_A` conflicts with an existing predefined constant")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("function `KEY_S` conflicts with an existing predefined constant")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("argument 1 to `key_down` expects `i32`, but found `bool`")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("function `key_pressed` expects 1 argument(s), but received 0")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("function `key_released` expects 1 argument(s), but received 2")
        }));
        assert!(errors.iter().any(|error| {
            error
                .message
                .contains("variable initializer requires a value, but found `void`")
        }));
    }
}
