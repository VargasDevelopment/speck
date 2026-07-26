use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::{
    BinaryOp, Block, Expr, ExprKind, Function, FunctionKind, Program, Stmt, StmtKind, Type, UnaryOp,
};

#[derive(Clone)]
struct Signature {
    return_type: Type,
    symbol: String,
}

#[derive(Clone)]
struct Variable {
    ty: Type,
    pointer: String,
}

#[derive(Clone)]
struct Value {
    ty: Type,
    repr: String,
}

pub fn emit(program: &Program) -> String {
    let functions = function_signatures(program);
    let globals: HashMap<_, _> = program
        .globals
        .iter()
        .map(|global| {
            (
                global.name.clone(),
                Variable {
                    ty: global.ty,
                    pointer: format!("@spk_global_{}", global.name),
                },
            )
        })
        .collect();

    let mut output = String::new();
    writeln!(
        output,
        "; Speck game: {}",
        program.title.replace(['\r', '\n'], " ")
    )
    .expect("writing to a string cannot fail");
    output.push_str("source_filename = \"speck\"\n\n");
    output.push_str("declare void @crumb_print_i32(i32)\n");
    output.push_str("declare void @crumb_debug_frame(i32, float)\n\n");

    for global in &program.globals {
        writeln!(
            output,
            "@spk_global_{} = internal global {} {}",
            global.name,
            llvm_type(global.ty),
            global_constant(&global.init)
        )
        .expect("writing to a string cannot fail");
    }
    if !program.globals.is_empty() {
        output.push('\n');
    }

    for function in &program.functions {
        let emitter = FunctionEmitter::new(function, &globals, &functions);
        output.push_str(&emitter.emit());
        output.push('\n');
    }
    if output.ends_with("\n\n") {
        output.pop();
    }
    output
}

fn function_signatures(program: &Program) -> HashMap<String, Signature> {
    let mut signatures = HashMap::from([
        (
            "print_i32".into(),
            Signature {
                return_type: Type::Void,
                symbol: "@crumb_print_i32".into(),
            },
        ),
        (
            "debug_frame".into(),
            Signature {
                return_type: Type::Void,
                symbol: "@crumb_debug_frame".into(),
            },
        ),
    ]);
    for function in &program.functions {
        if function.kind == FunctionKind::Named {
            signatures.insert(
                function.name.clone(),
                Signature {
                    return_type: function.return_type,
                    symbol: format!("@spk_fn_{}", function.name),
                },
            );
        }
    }
    signatures
}

struct FunctionEmitter<'a> {
    function: &'a Function,
    globals: &'a HashMap<String, Variable>,
    functions: &'a HashMap<String, Signature>,
    scopes: Vec<HashMap<String, Variable>>,
    lines: Vec<String>,
    next_temp: usize,
    next_label: usize,
    terminated: bool,
}

impl<'a> FunctionEmitter<'a> {
    fn new(
        function: &'a Function,
        globals: &'a HashMap<String, Variable>,
        functions: &'a HashMap<String, Signature>,
    ) -> Self {
        Self {
            function,
            globals,
            functions,
            scopes: vec![HashMap::new()],
            lines: Vec::new(),
            next_temp: 0,
            next_label: 0,
            terminated: false,
        }
    }

    fn emit(mut self) -> String {
        let params = self
            .function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| format!("{} %arg{index}", llvm_type(param.ty)))
            .collect::<Vec<_>>()
            .join(", ");
        let symbol = match self.function.kind {
            FunctionKind::Named => format!("spk_fn_{}", self.function.name),
            FunctionKind::Start => "spk_start".into(),
            FunctionKind::Update => "spk_update".into(),
            FunctionKind::Draw => "spk_draw".into(),
        };

        self.lines.push(format!(
            "define {} @{symbol}({params}) {{",
            llvm_type(self.function.return_type)
        ));
        self.lines.push("entry:".into());

        for (index, param) in self.function.params.iter().enumerate() {
            let pointer = self.temp();
            self.instruction(format!("{pointer} = alloca {}", llvm_type(param.ty)));
            self.instruction(format!(
                "store {} %arg{index}, ptr {pointer}",
                llvm_type(param.ty)
            ));
            self.scopes[0].insert(
                param.name.clone(),
                Variable {
                    ty: param.ty,
                    pointer,
                },
            );
        }

        self.statements(&self.function.body);
        if !self.terminated {
            if self.function.return_type == Type::Void {
                self.terminate("ret void".into());
            } else {
                self.terminate("unreachable".into());
            }
        }
        self.lines.push("}".into());
        self.lines.join("\n") + "\n"
    }

    fn statements(&mut self, block: &Block) {
        for statement in block {
            if self.terminated {
                break;
            }
            self.statement(statement);
        }
    }

    fn nested_block(&mut self, block: &Block) {
        self.scopes.push(HashMap::new());
        self.statements(block);
        self.scopes.pop();
    }

    fn statement(&mut self, statement: &Stmt) {
        match &statement.kind {
            StmtKind::Let { name, ty, init } => {
                let value = self.expression(init);
                let pointer = self.temp();
                self.instruction(format!("{pointer} = alloca {}", llvm_type(*ty)));
                self.instruction(format!(
                    "store {} {}, ptr {pointer}",
                    llvm_type(*ty),
                    value.repr
                ));
                self.scopes
                    .last_mut()
                    .expect("a function always has a local scope")
                    .insert(name.clone(), Variable { ty: *ty, pointer });
            }
            StmtKind::Assign { name, value } => {
                let variable = self
                    .variable(name)
                    .expect("semantic checking guarantees assignment targets");
                let value = self.expression(value);
                self.instruction(format!(
                    "store {} {}, ptr {}",
                    llvm_type(variable.ty),
                    value.repr,
                    variable.pointer
                ));
            }
            StmtKind::Expr(expression) => {
                self.expression(expression);
            }
            StmtKind::If {
                condition,
                then_block,
                else_block,
            } => self.if_statement(condition, then_block, else_block.as_ref()),
            StmtKind::While { condition, body } => self.while_statement(condition, body),
            StmtKind::Return(value) => {
                if let Some(value) = value {
                    let value = self.expression(value);
                    self.terminate(format!("ret {} {}", llvm_type(value.ty), value.repr));
                } else {
                    self.terminate("ret void".into());
                }
            }
        }
    }

    fn if_statement(&mut self, condition: &Expr, then_block: &Block, else_block: Option<&Block>) {
        let condition = self.expression(condition);
        let then_label = self.label("if_then");
        let end_label = self.label("if_end");

        if let Some(else_block) = else_block {
            let else_label = self.label("if_else");
            self.terminate(format!(
                "br i1 {}, label %{then_label}, label %{else_label}",
                condition.repr
            ));

            self.place_label(&then_label);
            self.nested_block(then_block);
            let then_terminated = self.terminated;
            if !then_terminated {
                self.terminate(format!("br label %{end_label}"));
            }

            self.place_label(&else_label);
            self.nested_block(else_block);
            let else_terminated = self.terminated;
            if !else_terminated {
                self.terminate(format!("br label %{end_label}"));
            }

            self.place_label(&end_label);
            if then_terminated && else_terminated {
                self.terminate("unreachable".into());
            }
        } else {
            self.terminate(format!(
                "br i1 {}, label %{then_label}, label %{end_label}",
                condition.repr
            ));
            self.place_label(&then_label);
            self.nested_block(then_block);
            if !self.terminated {
                self.terminate(format!("br label %{end_label}"));
            }
            self.place_label(&end_label);
        }
    }

    fn while_statement(&mut self, condition: &Expr, body: &Block) {
        let condition_label = self.label("while_condition");
        let body_label = self.label("while_body");
        let end_label = self.label("while_end");
        self.terminate(format!("br label %{condition_label}"));

        self.place_label(&condition_label);
        let condition = self.expression(condition);
        self.terminate(format!(
            "br i1 {}, label %{body_label}, label %{end_label}",
            condition.repr
        ));

        self.place_label(&body_label);
        self.nested_block(body);
        if !self.terminated {
            self.terminate(format!("br label %{condition_label}"));
        }

        self.place_label(&end_label);
    }

    fn expression(&mut self, expression: &Expr) -> Value {
        match &expression.kind {
            ExprKind::I32(value) => Value {
                ty: Type::I32,
                repr: value.to_string(),
            },
            ExprKind::F32(value) => Value {
                ty: Type::F32,
                repr: llvm_float(*value),
            },
            ExprKind::Bool(value) => Value {
                ty: Type::Bool,
                repr: value.to_string(),
            },
            ExprKind::Variable(name) => {
                let variable = self
                    .variable(name)
                    .expect("semantic checking guarantees variables");
                let temp = self.temp();
                self.instruction(format!(
                    "{temp} = load {}, ptr {}",
                    llvm_type(variable.ty),
                    variable.pointer
                ));
                Value {
                    ty: variable.ty,
                    repr: temp,
                }
            }
            ExprKind::Unary { op, operand } => {
                let operand = self.expression(operand);
                let temp = self.temp();
                let instruction = match (op, operand.ty) {
                    (UnaryOp::Negate, Type::I32) => {
                        format!("{temp} = sub i32 0, {}", operand.repr)
                    }
                    (UnaryOp::Negate, Type::F32) => {
                        format!("{temp} = fsub float {}, {}", llvm_float(0.0), operand.repr)
                    }
                    (UnaryOp::Not, Type::Bool) => {
                        format!("{temp} = xor i1 {}, true", operand.repr)
                    }
                    _ => unreachable!("semantic checking guarantees unary operand types"),
                };
                self.instruction(instruction);
                Value {
                    ty: operand.ty,
                    repr: temp,
                }
            }
            ExprKind::Binary { left, op, right } => {
                let left = self.expression(left);
                let right = self.expression(right);
                self.binary(left, *op, right)
            }
            ExprKind::Call { name, args } => {
                let signature = self
                    .functions
                    .get(name)
                    .expect("semantic checking guarantees called functions")
                    .clone();
                let args = args
                    .iter()
                    .map(|argument| {
                        let value = self.expression(argument);
                        format!("{} {}", llvm_type(value.ty), value.repr)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if signature.return_type == Type::Void {
                    self.instruction(format!("call void {}({args})", signature.symbol));
                    Value {
                        ty: Type::Void,
                        repr: String::new(),
                    }
                } else {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = call {} {}({args})",
                        llvm_type(signature.return_type),
                        signature.symbol
                    ));
                    Value {
                        ty: signature.return_type,
                        repr: temp,
                    }
                }
            }
        }
    }

    fn binary(&mut self, left: Value, op: BinaryOp, right: Value) -> Value {
        let temp = self.temp();
        let is_float = left.ty == Type::F32;
        let (instruction, result_type) = match op {
            BinaryOp::Add => (if is_float { "fadd" } else { "add" }, left.ty),
            BinaryOp::Subtract => (if is_float { "fsub" } else { "sub" }, left.ty),
            BinaryOp::Multiply => (if is_float { "fmul" } else { "mul" }, left.ty),
            BinaryOp::Divide => (if is_float { "fdiv" } else { "sdiv" }, left.ty),
            BinaryOp::Equal => (if is_float { "fcmp oeq" } else { "icmp eq" }, Type::Bool),
            BinaryOp::NotEqual => (if is_float { "fcmp one" } else { "icmp ne" }, Type::Bool),
            BinaryOp::Less => (if is_float { "fcmp olt" } else { "icmp slt" }, Type::Bool),
            BinaryOp::LessEqual => (if is_float { "fcmp ole" } else { "icmp sle" }, Type::Bool),
            BinaryOp::Greater => (if is_float { "fcmp ogt" } else { "icmp sgt" }, Type::Bool),
            BinaryOp::GreaterEqual => (if is_float { "fcmp oge" } else { "icmp sge" }, Type::Bool),
        };
        self.instruction(format!(
            "{temp} = {instruction} {} {}, {}",
            llvm_type(left.ty),
            left.repr,
            right.repr
        ));
        Value {
            ty: result_type,
            repr: temp,
        }
    }

    fn variable(&self, name: &str) -> Option<Variable> {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name).cloned())
            .or_else(|| self.globals.get(name).cloned())
    }

    fn temp(&mut self) -> String {
        let name = format!("%t{}", self.next_temp);
        self.next_temp += 1;
        name
    }

    fn label(&mut self, prefix: &str) -> String {
        let name = format!("{prefix}_{}", self.next_label);
        self.next_label += 1;
        name
    }

    fn instruction(&mut self, instruction: String) {
        self.lines.push(format!("  {instruction}"));
    }

    fn terminate(&mut self, instruction: String) {
        self.instruction(instruction);
        self.terminated = true;
    }

    fn place_label(&mut self, label: &str) {
        self.lines.push(format!("{label}:"));
        self.terminated = false;
    }
}

fn llvm_type(ty: Type) -> &'static str {
    match ty {
        Type::I32 => "i32",
        Type::F32 => "float",
        Type::Bool => "i1",
        Type::Void => "void",
    }
}

fn global_constant(expression: &Expr) -> String {
    match &expression.kind {
        ExprKind::I32(value) => value.to_string(),
        ExprKind::F32(value) => llvm_float(*value),
        ExprKind::Bool(value) => value.to_string(),
        ExprKind::Unary {
            op: UnaryOp::Negate,
            operand,
        } => match operand.kind {
            ExprKind::I32(value) => (-value).to_string(),
            ExprKind::F32(value) => llvm_float(-value),
            _ => unreachable!("semantic checking limits global initializers"),
        },
        _ => unreachable!("semantic checking limits global initializers"),
    }
}

fn llvm_float(value: f32) -> String {
    let scientific = format!("{value:.8e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("scientific float formatting always has an exponent");
    let exponent: i32 = exponent
        .parse()
        .expect("scientific float formatting has a numeric exponent");
    format!("{mantissa}e{exponent:+03}")
}

#[cfg(test)]
mod tests {
    use crate::{compile_to_llvm, lexer, parser, sema};

    #[test]
    fn emits_readable_ir_for_control_flow() {
        let source = r#"game "IR"
let frames: i32 = 0
fn add(a: i32, b: i32) -> i32 { return a + b }
start {}
update(dt: f32) {
    frames = add(frames, 1)
    if frames > 2 { frames = 0 }
}
draw { debug_frame(frames, 1.0) }
"#;
        let ir = compile_to_llvm(source).expect("compilation should pass");
        assert!(ir.contains("define i32 @spk_fn_add"));
        assert!(ir.contains("call i32 @spk_fn_add"));
        assert!(ir.contains("icmp sgt i32"));
        assert!(ir.contains("define void @spk_update(float %arg0)"));

        let tokens = lexer::lex(source).expect("lexing should pass");
        let program = parser::parse(tokens).expect("parsing should pass");
        sema::check(&program).expect("semantic checking should pass");
    }
}
