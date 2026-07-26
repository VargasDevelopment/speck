use std::collections::{HashMap, HashSet};

use crate::ast::{
    AssignOp, BinaryOp, Block, ConstantValue, Expr, ExprKind, Function, FunctionKind, Program,
    ReturnType, Stmt, StmtKind, UnaryOp, ValueType,
};
use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone)]
struct Signature {
    params: Vec<ValueType>,
    return_type: ReturnType,
}

#[derive(Clone, Copy)]
struct Binding {
    ty: ValueType,
    mutable: bool,
}

pub fn check(program: &mut Program) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut constants = HashMap::new();
    let mut globals = HashMap::new();
    let mut top_level_names: HashMap<String, &'static str> = builtins()
        .into_keys()
        .map(|name| (name, "function"))
        .collect();

    for constant in &program.constants {
        if constants
            .insert(constant.name.clone(), constant.ty)
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
        if globals.insert(global.name.clone(), global.ty).is_some() {
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

    let invalid_constants = program
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
    let invalid_globals = program
        .globals
        .iter()
        .enumerate()
        .filter_map(|(index, global)| {
            (!validate_compile_time_structure(
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
                params: function.params.iter().map(|param| param.ty).collect(),
                return_type: function.return_type,
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

    for constant in &program.constants {
        let mut checker = FunctionChecker::new(
            &globals,
            &constants,
            &functions,
            ReturnType::Void,
            &mut diagnostics,
        );
        if let Some(actual) = checker.require_value(&constant.init, "constant initializer")
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

    for global in &program.globals {
        let mut checker = FunctionChecker::new(
            &globals,
            &constants,
            &functions,
            ReturnType::Void,
            &mut diagnostics,
        );
        if let Some(actual) = checker.require_value(&global.init, "global initializer")
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
                    ty: constant.ty,
                    init: constant.init.clone(),
                    span: constant.span,
                },
            )
        })
        .collect();
    let mut evaluator = ConstantEvaluator::new(constant_defs, &globals, &invalid_constants);
    let constant_names = program
        .constants
        .iter()
        .map(|constant| constant.name.clone())
        .collect::<Vec<_>>();
    for name in constant_names {
        if invalid_constants.contains(&name) {
            continue;
        }
        if let Err(error) = evaluator.evaluate_named(&name) {
            diagnostics.push(error);
        }
    }
    let evaluated_constants = evaluator.values.clone();
    for constant in &mut program.constants {
        constant.value = evaluated_constants.get(&constant.name).copied();
    }

    for (index, global) in program.globals.iter_mut().enumerate() {
        if invalid_globals.contains(&index) {
            continue;
        }
        match evaluate_initializer(&global.init, &evaluated_constants, &globals) {
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
        check_function(function, &globals, &constants, &functions, &mut diagnostics);
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
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
        ExprKind::I32(_) | ExprKind::F32(_) | ExprKind::Bool(_) | ExprKind::Variable(_) => {}
    }
    valid
}

fn builtins() -> HashMap<String, Signature> {
    HashMap::from([
        (
            "print_i32".into(),
            Signature {
                params: vec![ValueType::I32],
                return_type: ReturnType::Void,
            },
        ),
        (
            "debug_frame".into(),
            Signature {
                params: vec![ValueType::I32, ValueType::F32],
                return_type: ReturnType::Void,
            },
        ),
        (
            "clear_rgb".into(),
            Signature {
                params: vec![ValueType::I32, ValueType::I32, ValueType::I32],
                return_type: ReturnType::Void,
            },
        ),
        (
            "fill_rect".into(),
            Signature {
                params: vec![ValueType::I32; 7],
                return_type: ReturnType::Void,
            },
        ),
    ])
}

fn check_function(
    function: &Function,
    globals: &HashMap<String, ValueType>,
    constants: &HashMap<String, ValueType>,
    functions: &HashMap<String, Signature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut checker = FunctionChecker::new(
        globals,
        constants,
        functions,
        function.return_type,
        diagnostics,
    );
    for param in &function.params {
        if checker.scopes[0]
            .insert(
                param.name.clone(),
                Binding {
                    ty: param.ty,
                    mutable: true,
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
    functions: &'a HashMap<String, Signature>,
    return_type: ReturnType,
    scopes: Vec<HashMap<String, Binding>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> FunctionChecker<'a> {
    fn new(
        globals: &'a HashMap<String, ValueType>,
        constants: &'a HashMap<String, ValueType>,
        functions: &'a HashMap<String, Signature>,
        return_type: ReturnType,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        Self {
            globals,
            constants,
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
                if let Some(actual) = self.require_value(init, "variable initializer")
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
                            ty: *ty,
                            mutable: true,
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
            StmtKind::Assign { name, op, value } => {
                let binding = self.binding(name);
                let actual = self.require_value(value, "assignment");
                let Some(binding) = binding else {
                    self.error(
                        format!("cannot assign to unknown variable `{name}`"),
                        statement.span,
                    );
                    return;
                };
                if !binding.mutable {
                    let message = if *op == AssignOp::Set {
                        format!("cannot assign to constant `{name}`")
                    } else {
                        format!("cannot use compound assignment on constant `{name}`")
                    };
                    self.error(message, statement.span);
                    return;
                }
                if *op != AssignOp::Set && !binding.ty.is_numeric() {
                    self.error(
                        format!(
                            "compound assignment requires a numeric target, but `{name}` is `{}`",
                            binding.ty.name()
                        ),
                        statement.span,
                    );
                }
                if let Some(actual) = actual
                    && binding.ty != actual
                {
                    let message = if *op == AssignOp::Set {
                        format!(
                            "cannot assign `{}` to variable `{name}` of type `{}`",
                            actual.name(),
                            binding.ty.name()
                        )
                    } else {
                        format!(
                            "compound assignment to `{name}` expects `{}`, but found `{}`",
                            binding.ty.name(),
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
            StmtKind::Return(value) => match (self.return_type, value) {
                (ReturnType::Void, None) => {}
                (ReturnType::Void, Some(value)) => {
                    self.infer_expr(value);
                    self.error("void function cannot return a value", value.span);
                }
                (ReturnType::Value(expected), None) => self.error(
                    format!("return requires a value of type `{}`", expected.name()),
                    statement.span,
                ),
                (ReturnType::Value(expected), Some(value)) => match self.infer_expr(value) {
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
                },
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
            ExprKind::Variable(name) => match self.binding(name) {
                Some(binding) => Some(binding.ty.into()),
                None => {
                    self.error(format!("unknown variable `{name}`"), expression.span);
                    None
                }
            },
            ExprKind::Unary { op, operand } => {
                if *op == UnaryOp::Negate
                    && matches!(operand.kind, ExprKind::I32(value) if value == 2_147_483_648)
                {
                    return Some(ValueType::I32.into());
                }
                let ty = self.require_value(operand, "unary operator")?;
                match (op, ty) {
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
                    BinaryOp::Equal | BinaryOp::NotEqual => Some(ValueType::Bool.into()),
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
                        (signature.params.get(index).copied(), actual)
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
                        Some((*target).into())
                    }
                    Some(ReturnType::Value(ValueType::Bool)) => {
                        self.error("cannot convert `bool` to a numeric type", args[0].span);
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
            .find_map(|scope| scope.get(name).copied())
            .or_else(|| {
                self.globals
                    .get(name)
                    .copied()
                    .map(|ty| Binding { ty, mutable: true })
            })
            .or_else(|| {
                self.constants
                    .get(name)
                    .copied()
                    .map(|ty| Binding { ty, mutable: false })
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
    invalid: &'a HashSet<String>,
    states: HashMap<String, VisitState>,
    values: HashMap<String, ConstantValue>,
    stack: Vec<String>,
}

impl<'a> ConstantEvaluator<'a> {
    fn new(
        definitions: HashMap<String, ConstantDefinition>,
        globals: &'a HashMap<String, ValueType>,
        invalid: &'a HashSet<String>,
    ) -> Self {
        Self {
            definitions,
            globals,
            invalid,
            states: HashMap::new(),
            values: HashMap::new(),
            stack: Vec::new(),
        }
    }

    fn evaluate_named(&mut self, name: &str) -> Result<ConstantValue, Diagnostic> {
        if let Some(value) = self.values.get(name).copied() {
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
        let result = self.evaluate_expr(&definition.init).and_then(|value| {
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
                self.values.insert(name.to_owned(), value);
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

fn evaluate_initializer(
    expression: &Expr,
    constants: &HashMap<String, ConstantValue>,
    globals: &HashMap<String, ValueType>,
) -> Result<ConstantValue, String> {
    let mut lookup = |name: &str| {
        if let Some(value) = constants.get(name).copied() {
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
    evaluate_expression(expression, &mut lookup).map_err(|diagnostic| diagnostic.message)
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
            ExprKind::Variable(name) => constant(name),
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
                convert_constant(*target, source)
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
}
