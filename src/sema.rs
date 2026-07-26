use std::collections::HashMap;

use crate::ast::{
    BinaryOp, Block, Expr, ExprKind, Function, FunctionKind, Program, Stmt, StmtKind, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, Span};

#[derive(Clone)]
struct Signature {
    params: Vec<Type>,
    return_type: Type,
}

pub fn check(program: &Program) -> Result<(), Vec<Diagnostic>> {
    let mut diagnostics = Vec::new();
    let mut globals = HashMap::new();
    for global in &program.globals {
        if globals.insert(global.name.clone(), global.ty).is_some() {
            diagnostics.push(Diagnostic::new(
                format!("global `{}` is declared more than once", global.name),
                global.span,
            ));
        }
    }

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
            if globals.contains_key(&function.name) {
                diagnostics.push(Diagnostic::new(
                    format!(
                        "function `{}` conflicts with a global variable of the same name",
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

    for global in &program.globals {
        let mut checker = FunctionChecker::new(&globals, &functions, Type::Void, &mut diagnostics);
        let actual = checker.infer_expr(&global.init);
        if let Some(actual) = actual
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
        if !is_global_constant(&global.init) {
            checker.error(
                "global initializers are currently limited to literal constants",
                global.init.span,
            );
        }
    }

    for function in &program.functions {
        if function.kind == FunctionKind::Update
            && (function.params.len() != 1 || function.params[0].ty != Type::F32)
        {
            diagnostics.push(Diagnostic::new(
                "`update` must take exactly one `f32` frame-delta parameter",
                function.span,
            ));
        }
        check_function(function, &globals, &functions, &mut diagnostics);
    }

    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn builtins() -> HashMap<String, Signature> {
    HashMap::from([
        (
            "print_i32".into(),
            Signature {
                params: vec![Type::I32],
                return_type: Type::Void,
            },
        ),
        (
            "debug_frame".into(),
            Signature {
                params: vec![Type::I32, Type::F32],
                return_type: Type::Void,
            },
        ),
    ])
}

fn check_function(
    function: &Function,
    globals: &HashMap<String, Type>,
    functions: &HashMap<String, Signature>,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let mut checker = FunctionChecker::new(globals, functions, function.return_type, diagnostics);
    for param in &function.params {
        if checker.scopes[0]
            .insert(param.name.clone(), param.ty)
            .is_some()
        {
            checker.error(
                format!("parameter `{}` is declared more than once", param.name),
                param.span,
            );
        }
    }
    checker.check_statements(&function.body);
    if function.return_type != Type::Void && !block_returns(&function.body) {
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
    globals: &'a HashMap<String, Type>,
    functions: &'a HashMap<String, Signature>,
    return_type: Type,
    scopes: Vec<HashMap<String, Type>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl<'a> FunctionChecker<'a> {
    fn new(
        globals: &'a HashMap<String, Type>,
        functions: &'a HashMap<String, Signature>,
        return_type: Type,
        diagnostics: &'a mut Vec<Diagnostic>,
    ) -> Self {
        Self {
            globals,
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
                let actual = self.infer_expr(init);
                if let Some(actual) = actual
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
                if scope.insert(name.clone(), *ty).is_some() {
                    self.error(
                        format!("variable `{name}` is declared more than once in this block"),
                        statement.span,
                    );
                }
            }
            StmtKind::Assign { name, value } => {
                let expected = self.variable_type(name);
                let actual = self.infer_expr(value);
                match (expected, actual) {
                    (None, _) => self.error(
                        format!("cannot assign to unknown variable `{name}`"),
                        statement.span,
                    ),
                    (Some(expected), Some(actual)) if expected != actual => self.error(
                        format!(
                            "cannot assign `{}` to variable `{name}` of type `{}`",
                            actual.name(),
                            expected.name()
                        ),
                        value.span,
                    ),
                    _ => {}
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
                (Type::Void, None) => {}
                (Type::Void, Some(value)) => {
                    self.infer_expr(value);
                    self.error("game entry points cannot return a value", value.span);
                }
                (expected, None) => self.error(
                    format!("return requires a value of type `{}`", expected.name()),
                    statement.span,
                ),
                (expected, Some(value)) => {
                    let actual = self.infer_expr(value);
                    if let Some(actual) = actual
                        && actual != expected
                    {
                        self.error(
                            format!(
                                "return expects `{}`, but found `{}`",
                                expected.name(),
                                actual.name()
                            ),
                            value.span,
                        );
                    }
                }
            },
        }
    }

    fn infer_expr(&mut self, expression: &Expr) -> Option<Type> {
        match &expression.kind {
            ExprKind::I32(value) => {
                if i32::try_from(*value).is_err() {
                    self.error("integer literal does not fit in `i32`", expression.span);
                }
                Some(Type::I32)
            }
            ExprKind::F32(value) => {
                if !value.is_finite() {
                    self.error("floating-point literal must be finite", expression.span);
                }
                Some(Type::F32)
            }
            ExprKind::Bool(_) => Some(Type::Bool),
            ExprKind::Variable(name) => match self.variable_type(name) {
                Some(ty) => Some(ty),
                None => {
                    self.error(format!("unknown variable `{name}`"), expression.span);
                    None
                }
            },
            ExprKind::Unary { op, operand } => {
                let ty = self.infer_expr(operand)?;
                match (op, ty) {
                    (UnaryOp::Negate, Type::I32 | Type::F32) => Some(ty),
                    (UnaryOp::Not, Type::Bool) => Some(Type::Bool),
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
                let left_type = self.infer_expr(left);
                let right_type = self.infer_expr(right);
                let (Some(left_type), Some(right_type)) = (left_type, right_type) else {
                    return None;
                };
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
                        if matches!(left_type, Type::I32 | Type::F32) {
                            Some(left_type)
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
                        if matches!(left_type, Type::I32 | Type::F32) {
                            Some(Type::Bool)
                        } else {
                            self.error(
                                "ordering comparison requires numeric operands",
                                expression.span,
                            );
                            None
                        }
                    }
                    BinaryOp::Equal | BinaryOp::NotEqual => {
                        if left_type == Type::Void {
                            self.error("cannot compare values of type `void`", expression.span);
                            None
                        } else {
                            Some(Type::Bool)
                        }
                    }
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
                    let actual = self.infer_expr(arg);
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
        }
    }

    fn require_bool(&mut self, expression: &Expr, description: &str) {
        if let Some(actual) = self.infer_expr(expression)
            && actual != Type::Bool
        {
            self.error(
                format!("{description} must be `bool`, found `{}`", actual.name()),
                expression.span,
            );
        }
    }

    fn variable_type(&self, name: &str) -> Option<Type> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).copied())
            .or_else(|| self.globals.get(name).copied())
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        self.diagnostics.push(Diagnostic::new(message, span));
    }
}

fn is_global_constant(expression: &Expr) -> bool {
    matches!(
        expression.kind,
        ExprKind::I32(_) | ExprKind::F32(_) | ExprKind::Bool(_)
    ) || matches!(
        &expression.kind,
        ExprKind::Unary {
            op: UnaryOp::Negate,
            operand,
        } if matches!(operand.kind, ExprKind::I32(_) | ExprKind::F32(_))
    )
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
        let program = parser::parse(tokens).expect("parsing should pass");
        check(&program).expect_err("semantic checking should fail")
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
    fn rejects_mismatched_assignment() {
        let errors =
            errors("game \"Bad\"\nlet n: i32 = 1\nstart { n = 1.0 }\nupdate(dt: f32) {}\ndraw {}");
        assert!(
            errors
                .iter()
                .any(|error| error.message.contains("cannot assign `f32`"))
        );
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
}
