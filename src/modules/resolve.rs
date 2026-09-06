use std::collections::{HashMap, HashSet};

use crate::ast::{
    ArrayLength, Block, Expr, ExprKind, FunctionKind, ReturnType, StmtKind, ValueType,
};
use crate::builtins;
use crate::diagnostic::{Diagnostic, Span};

use super::Module;

struct Namespace {
    declarations: HashMap<String, String>,
    aliases: HashMap<String, usize>,
}

pub(super) fn resolve(modules: &mut [Module]) -> Result<(), Vec<Diagnostic>> {
    // Preserve the existing single-file semantic path, including its diagnostics.
    if modules.len() == 1 && modules[0].imports.is_empty() {
        return Ok(());
    }
    let mut diagnostics = Vec::new();
    let namespaces = modules
        .iter()
        .map(|module| {
            let mut declarations = HashMap::new();
            let program = &module.program;
            let names = program
                .structs
                .iter()
                .map(|item| (&item.name, item.span))
                .chain(program.constants.iter().map(|item| (&item.name, item.span)))
                .chain(program.globals.iter().map(|item| (&item.name, item.span)))
                .chain(
                    program
                        .functions
                        .iter()
                        .filter(|item| item.kind == FunctionKind::Named)
                        .map(|item| (&item.name, item.span)),
                );
            for (name, span) in names {
                if declarations
                    .insert(name.clone(), qualify(&module.qualifier, name))
                    .is_some()
                {
                    diagnostics.push(Diagnostic::new(
                        format!("declaration `{name}` is declared more than once"),
                        span,
                    ));
                }
                if is_builtin(name) {
                    diagnostics.push(Diagnostic::new(
                        format!("declaration `{name}` conflicts with a predefined name"),
                        span,
                    ));
                }
            }
            let mut aliases = HashMap::new();
            for (import, target) in &module.imports {
                if declarations.contains_key(&import.alias) || is_builtin(&import.alias) {
                    diagnostics.push(Diagnostic::new(
                        format!(
                            "import alias `{}` conflicts with an existing declaration",
                            import.alias
                        ),
                        import.span,
                    ));
                }
                aliases.insert(import.alias.clone(), *target);
            }
            Namespace {
                declarations,
                aliases,
            }
        })
        .collect::<Vec<_>>();
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }
    for (index, module) in modules.iter_mut().enumerate() {
        let mut resolver = Resolver {
            namespaces: &namespaces,
            module: index,
            scopes: Vec::new(),
            diagnostics: &mut diagnostics,
        };
        let program = &mut module.program;
        for declaration in &mut program.structs {
            declaration.name = namespaces[index].declarations[&declaration.name].clone();
            for field in &mut declaration.fields {
                resolver.ty(&mut field.ty, field.span);
            }
        }
        for constant in &mut program.constants {
            constant.name = namespaces[index].declarations[&constant.name].clone();
            resolver.ty(&mut constant.ty, constant.span);
            resolver.expression(&mut constant.init);
        }
        for global in &mut program.globals {
            global.name = namespaces[index].declarations[&global.name].clone();
            resolver.ty(&mut global.ty, global.span);
            resolver.expression(&mut global.init);
        }
        for function in &mut program.functions {
            if function.kind == FunctionKind::Named {
                function.name = namespaces[index].declarations[&function.name].clone();
            }
            for param in &mut function.params {
                resolver.ty(&mut param.ty, param.span);
            }
            if let ReturnType::Value(ty) = &mut function.return_type {
                resolver.ty(ty, function.span);
            }
            resolver.scopes.push(
                function
                    .params
                    .iter()
                    .map(|param| param.name.clone())
                    .collect(),
            );
            // Parameters and top-level function locals share a semantic scope.
            resolver.statements(&mut function.body);
            resolver.scopes.pop();
        }
    }
    if diagnostics.is_empty() {
        Ok(())
    } else {
        Err(diagnostics)
    }
}

fn qualify(module: &str, name: &str) -> String {
    if module.is_empty() {
        name.to_owned()
    } else {
        format!("{module}::{name}")
    }
}

fn is_builtin(name: &str) -> bool {
    builtins::FUNCTIONS.iter().any(|item| item.name == name)
        || builtins::CONSTANTS.iter().any(|item| item.name == name)
}

struct Resolver<'a> {
    namespaces: &'a [Namespace],
    module: usize,
    scopes: Vec<HashSet<String>>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl Resolver<'_> {
    fn name(&mut self, name: &mut String, span: Span, description: &str, local: bool) {
        if local && self.scopes.iter().rev().any(|scope| scope.contains(name)) {
            return;
        }
        let namespace = &self.namespaces[self.module];
        if let Some((alias, member)) = name.split_once("::") {
            let Some(target) = namespace.aliases.get(alias) else {
                self.diagnostics.push(Diagnostic::new(
                    format!("unknown import alias `{alias}`"),
                    span,
                ));
                return;
            };
            if let Some(resolved) = self.namespaces[*target].declarations.get(member) {
                *name = resolved.clone();
            } else {
                self.diagnostics.push(Diagnostic::new(
                    format!("module `{alias}` has no declaration `{member}`"),
                    span,
                ));
            }
        } else if let Some(resolved) = namespace.declarations.get(name) {
            *name = resolved.clone();
        } else if !is_builtin(name) {
            // Never leave an unresolved name for the flattened program to bind
            // accidentally to a declaration belonging to the entry file.
            self.diagnostics.push(Diagnostic::new(
                format!("unknown {description} `{name}`"),
                span,
            ));
        }
    }

    fn ty(&mut self, ty: &mut ValueType, span: Span) {
        match ty {
            ValueType::Struct(name) => self.name(name, span, "struct type", false),
            ValueType::Array { element, length } => {
                self.ty(element, span);
                if let ArrayLength::Constant { name, span } = length {
                    self.name(name, *span, "constant", false);
                }
            }
            ValueType::I32 | ValueType::F32 | ValueType::Bool => {}
        }
    }

    fn block(&mut self, block: &mut Block) {
        self.scopes.push(HashSet::new());
        self.statements(block);
        self.scopes.pop();
    }

    fn statements(&mut self, block: &mut Block) {
        for statement in block {
            match &mut statement.kind {
                StmtKind::Let { name, ty, init } => {
                    self.ty(ty, statement.span);
                    self.expression(init);
                    self.scopes.last_mut().unwrap().insert(name.clone());
                }
                StmtKind::Assign { target, value, .. } => {
                    self.expression(target);
                    self.expression(value);
                }
                StmtKind::Expr(expr) => self.expression(expr),
                StmtKind::If {
                    condition,
                    then_block,
                    else_block,
                } => {
                    self.expression(condition);
                    self.block(then_block);
                    if let Some(block) = else_block {
                        self.block(block);
                    }
                }
                StmtKind::While { condition, body } => {
                    self.expression(condition);
                    self.block(body);
                }
                StmtKind::For {
                    name,
                    lower,
                    upper,
                    body,
                    ..
                } => {
                    self.expression(lower);
                    self.expression(upper);
                    self.scopes.push(HashSet::from([name.clone()]));
                    self.statements(body);
                    self.scopes.pop();
                }
                StmtKind::Return(expr) => {
                    if let Some(expr) = expr {
                        self.expression(expr);
                    }
                }
            }
        }
    }

    fn expression(&mut self, expression: &mut Expr) {
        match &mut expression.kind {
            ExprKind::Variable(name) => self.name(name, expression.span, "variable", true),
            ExprKind::Call { name, args } => {
                self.name(name, expression.span, "function", false);
                for arg in args {
                    self.expression(arg);
                }
            }
            ExprKind::StructLiteral { name, fields } => {
                self.name(name, expression.span, "struct type", false);
                for field in fields {
                    self.expression(&mut field.value);
                }
            }
            ExprKind::ArrayLiteral(elements) => {
                for element in elements {
                    self.expression(element);
                }
            }
            ExprKind::Index { base, index } => {
                self.expression(base);
                self.expression(index);
            }
            ExprKind::Field { base, .. } => self.expression(base),
            ExprKind::Unary { operand, .. } => self.expression(operand),
            ExprKind::Binary { left, right, .. } => {
                self.expression(left);
                self.expression(right);
            }
            ExprKind::Conversion { target, args } => {
                self.ty(target, expression.span);
                for arg in args {
                    self.expression(arg);
                }
            }
            ExprKind::I32(_) | ExprKind::F32(_) | ExprKind::Bool(_) => {}
        }
    }
}
