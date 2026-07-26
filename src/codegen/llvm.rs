use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::{
    AssignOp, BinaryOp, Block, ConstantValue, Expr, ExprKind, Function, FunctionKind, Program,
    ReturnType, Stmt, StmtKind, UnaryOp, ValueType,
};

#[derive(Clone)]
struct Signature {
    return_type: ReturnType,
    symbol: String,
}

#[derive(Clone)]
struct Variable {
    ty: ValueType,
    pointer: String,
}

#[derive(Clone)]
struct Value {
    ty: ReturnType,
    repr: String,
}

impl Value {
    fn value_type(&self) -> ValueType {
        self.ty
            .value_type()
            .expect("semantic checking guarantees a value in this context")
    }
}

pub fn emit(program: &Program) -> String {
    emit_for_target(program, None)
}

pub fn emit_for_target(program: &Program, target_triple: Option<&str>) -> String {
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
    let constants = program
        .constants
        .iter()
        .map(|constant| {
            (
                constant.name.clone(),
                constant
                    .value
                    .expect("semantic checking evaluates every constant"),
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
    output.push_str("source_filename = \"speck\"\n");
    if let Some(target_triple) = target_triple {
        writeln!(output, "target triple = \"{target_triple}\"")
            .expect("writing to a string cannot fail");
    }
    output.push('\n');
    output.push_str("declare void @crumb_print_i32(i32)\n");
    output.push_str("declare void @crumb_debug_frame(i32, float)\n");
    output.push_str("declare void @crumb_clear_rgb(i32, i32, i32)\n");
    output.push_str("declare void @crumb_fill_rect(i32, i32, i32, i32, i32, i32, i32)\n\n");

    for global in &program.globals {
        let value = global
            .value
            .expect("semantic checking evaluates every global initializer");
        writeln!(
            output,
            "@spk_global_{} = internal global {} {}",
            global.name,
            llvm_value_type(global.ty),
            llvm_constant(value)
        )
        .expect("writing to a string cannot fail");
    }
    if !program.globals.is_empty() {
        output.push('\n');
    }

    for function in &program.functions {
        let emitter = FunctionEmitter::new(function, &globals, &constants, &functions);
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
                return_type: ReturnType::Void,
                symbol: "@crumb_print_i32".into(),
            },
        ),
        (
            "debug_frame".into(),
            Signature {
                return_type: ReturnType::Void,
                symbol: "@crumb_debug_frame".into(),
            },
        ),
        (
            "clear_rgb".into(),
            Signature {
                return_type: ReturnType::Void,
                symbol: "@crumb_clear_rgb".into(),
            },
        ),
        (
            "fill_rect".into(),
            Signature {
                return_type: ReturnType::Void,
                symbol: "@crumb_fill_rect".into(),
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
    constants: &'a HashMap<String, ConstantValue>,
    functions: &'a HashMap<String, Signature>,
    scopes: Vec<HashMap<String, Variable>>,
    lines: Vec<String>,
    next_temp: usize,
    next_label: usize,
    current_block: String,
    terminated: bool,
}

impl<'a> FunctionEmitter<'a> {
    fn new(
        function: &'a Function,
        globals: &'a HashMap<String, Variable>,
        constants: &'a HashMap<String, ConstantValue>,
        functions: &'a HashMap<String, Signature>,
    ) -> Self {
        Self {
            function,
            globals,
            constants,
            functions,
            scopes: vec![HashMap::new()],
            lines: Vec::new(),
            next_temp: 0,
            next_label: 0,
            current_block: "entry".into(),
            terminated: false,
        }
    }

    fn emit(mut self) -> String {
        let params = self
            .function
            .params
            .iter()
            .enumerate()
            .map(|(index, param)| format!("{} %arg{index}", llvm_value_type(param.ty)))
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
            llvm_return_type(self.function.return_type)
        ));
        self.lines.push("entry:".into());

        for (index, param) in self.function.params.iter().enumerate() {
            let pointer = self.temp();
            self.instruction(format!("{pointer} = alloca {}", llvm_value_type(param.ty)));
            self.instruction(format!(
                "store {} %arg{index}, ptr {pointer}",
                llvm_value_type(param.ty)
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
            if self.function.return_type == ReturnType::Void {
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
                self.instruction(format!("{pointer} = alloca {}", llvm_value_type(*ty)));
                self.instruction(format!(
                    "store {} {}, ptr {pointer}",
                    llvm_value_type(*ty),
                    value.repr
                ));
                self.scopes
                    .last_mut()
                    .expect("a function always has a local scope")
                    .insert(name.clone(), Variable { ty: *ty, pointer });
            }
            StmtKind::Assign { name, op, value } => {
                let variable = self
                    .variable(name)
                    .expect("semantic checking guarantees assignment targets");
                let value = if *op == AssignOp::Set {
                    self.expression(value)
                } else {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = load {}, ptr {}",
                        llvm_value_type(variable.ty),
                        variable.pointer
                    ));
                    let left = Value {
                        ty: variable.ty.into(),
                        repr: temp,
                    };
                    let right = self.expression(value);
                    let binary_op = match op {
                        AssignOp::Add => BinaryOp::Add,
                        AssignOp::Subtract => BinaryOp::Subtract,
                        AssignOp::Multiply => BinaryOp::Multiply,
                        AssignOp::Divide => BinaryOp::Divide,
                        AssignOp::Set => unreachable!(),
                    };
                    self.binary(left, binary_op, right)
                };
                self.instruction(format!(
                    "store {} {}, ptr {}",
                    llvm_value_type(variable.ty),
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
                    self.terminate(format!(
                        "ret {} {}",
                        llvm_value_type(value.value_type()),
                        value.repr
                    ));
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
                ty: ValueType::I32.into(),
                repr: value.to_string(),
            },
            ExprKind::F32(value) => Value {
                ty: ValueType::F32.into(),
                repr: llvm_float(*value),
            },
            ExprKind::Bool(value) => Value {
                ty: ValueType::Bool.into(),
                repr: value.to_string(),
            },
            ExprKind::Variable(name) => {
                if let Some(variable) = self.variable(name) {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = load {}, ptr {}",
                        llvm_value_type(variable.ty),
                        variable.pointer
                    ));
                    Value {
                        ty: variable.ty.into(),
                        repr: temp,
                    }
                } else {
                    let value = self
                        .constants
                        .get(name)
                        .copied()
                        .expect("semantic checking guarantees constant references");
                    Value {
                        ty: value.ty().into(),
                        repr: llvm_constant(value),
                    }
                }
            }
            ExprKind::Unary { op, operand } => {
                if *op == UnaryOp::Negate
                    && matches!(operand.kind, ExprKind::I32(value) if value == 2_147_483_648)
                {
                    return Value {
                        ty: ValueType::I32.into(),
                        repr: i32::MIN.to_string(),
                    };
                }
                let operand = self.expression(operand);
                let operand_type = operand.value_type();
                let temp = self.temp();
                let instruction = match (op, operand_type) {
                    (UnaryOp::Negate, ValueType::I32) => {
                        format!("{temp} = sub i32 0, {}", operand.repr)
                    }
                    (UnaryOp::Negate, ValueType::F32) => {
                        format!("{temp} = fsub float {}, {}", llvm_float(0.0), operand.repr)
                    }
                    (UnaryOp::Not, ValueType::Bool) => {
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
                if matches!(op, BinaryOp::LogicalAnd | BinaryOp::LogicalOr) {
                    self.logical_expression(left, *op, right)
                } else {
                    let left = self.expression(left);
                    let right = self.expression(right);
                    self.binary(left, *op, right)
                }
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
                        format!("{} {}", llvm_value_type(value.value_type()), value.repr)
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                if signature.return_type == ReturnType::Void {
                    self.instruction(format!("call void {}({args})", signature.symbol));
                    Value {
                        ty: ReturnType::Void,
                        repr: String::new(),
                    }
                } else {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = call {} {}({args})",
                        llvm_return_type(signature.return_type),
                        signature.symbol
                    ));
                    Value {
                        ty: signature.return_type,
                        repr: temp,
                    }
                }
            }
            ExprKind::Conversion { target, args } => {
                let source = self.expression(&args[0]);
                let source_type = source.value_type();
                match (source_type, *target) {
                    (ValueType::I32, ValueType::I32) | (ValueType::F32, ValueType::F32) => source,
                    (ValueType::I32, ValueType::F32) => {
                        let temp = self.temp();
                        self.instruction(format!("{temp} = sitofp i32 {} to float", source.repr));
                        Value {
                            ty: ValueType::F32.into(),
                            repr: temp,
                        }
                    }
                    (ValueType::F32, ValueType::I32) => self.safe_float_to_int(source),
                    _ => unreachable!("semantic checking guarantees numeric conversions"),
                }
            }
        }
    }

    fn logical_expression(&mut self, left: &Expr, op: BinaryOp, right: &Expr) -> Value {
        let left = self.expression(left);
        let rhs_label = self.label("logical_rhs");
        let short_label = self.label("logical_short");
        let merge_label = self.label("logical_merge");
        let short_value = if op == BinaryOp::LogicalAnd {
            "false"
        } else {
            "true"
        };
        let branch = if op == BinaryOp::LogicalAnd {
            format!(
                "br i1 {}, label %{rhs_label}, label %{short_label}",
                left.repr
            )
        } else {
            format!(
                "br i1 {}, label %{short_label}, label %{rhs_label}",
                left.repr
            )
        };
        self.terminate(branch);

        self.place_label(&short_label);
        self.terminate(format!("br label %{merge_label}"));

        self.place_label(&rhs_label);
        let right = self.expression(right);
        let rhs_predecessor = self.current_block.clone();
        self.terminate(format!("br label %{merge_label}"));

        self.place_label(&merge_label);
        let temp = self.temp();
        self.instruction(format!(
            "{temp} = phi i1 [{short_value}, %{short_label}], [{}, %{rhs_predecessor}]",
            right.repr
        ));
        Value {
            ty: ValueType::Bool.into(),
            repr: temp,
        }
    }

    fn safe_float_to_int(&mut self, source: Value) -> Value {
        let nan_label = self.label("fptosi_nan");
        let high_check_label = self.label("fptosi_high_check");
        let high_label = self.label("fptosi_high");
        let low_check_label = self.label("fptosi_low_check");
        let low_label = self.label("fptosi_low");
        let convert_label = self.label("fptosi_convert");
        let merge_label = self.label("fptosi_merge");

        let is_nan = self.temp();
        self.instruction(format!("{is_nan} = fcmp uno float {0}, {0}", source.repr));
        self.terminate(format!(
            "br i1 {is_nan}, label %{nan_label}, label %{high_check_label}"
        ));

        self.place_label(&nan_label);
        self.terminate(format!("br label %{merge_label}"));

        self.place_label(&high_check_label);
        let is_high = self.temp();
        self.instruction(format!(
            "{is_high} = fcmp oge float {}, 0x41E0000000000000",
            source.repr
        ));
        self.terminate(format!(
            "br i1 {is_high}, label %{high_label}, label %{low_check_label}"
        ));

        self.place_label(&high_label);
        self.terminate(format!("br label %{merge_label}"));

        self.place_label(&low_check_label);
        let is_low = self.temp();
        self.instruction(format!(
            "{is_low} = fcmp ole float {}, 0xC1E0000000000000",
            source.repr
        ));
        self.terminate(format!(
            "br i1 {is_low}, label %{low_label}, label %{convert_label}"
        ));

        self.place_label(&low_label);
        self.terminate(format!("br label %{merge_label}"));

        self.place_label(&convert_label);
        let converted = self.temp();
        self.instruction(format!("{converted} = fptosi float {} to i32", source.repr));
        self.terminate(format!("br label %{merge_label}"));

        self.place_label(&merge_label);
        let result = self.temp();
        self.instruction(format!(
            "{result} = phi i32 [0, %{nan_label}], [{}, %{high_label}], [{}, %{low_label}], [{converted}, %{convert_label}]",
            i32::MAX,
            i32::MIN
        ));
        Value {
            ty: ValueType::I32.into(),
            repr: result,
        }
    }

    fn binary(&mut self, left: Value, op: BinaryOp, right: Value) -> Value {
        let temp = self.temp();
        let left_type = left.value_type();
        let is_float = left_type == ValueType::F32;
        let (instruction, result_type) = match op {
            BinaryOp::Add => (if is_float { "fadd" } else { "add" }, left_type),
            BinaryOp::Subtract => (if is_float { "fsub" } else { "sub" }, left_type),
            BinaryOp::Multiply => (if is_float { "fmul" } else { "mul" }, left_type),
            BinaryOp::Divide => (if is_float { "fdiv" } else { "sdiv" }, left_type),
            BinaryOp::Equal => (
                if is_float { "fcmp oeq" } else { "icmp eq" },
                ValueType::Bool,
            ),
            BinaryOp::NotEqual => (
                if is_float { "fcmp one" } else { "icmp ne" },
                ValueType::Bool,
            ),
            BinaryOp::Less => (
                if is_float { "fcmp olt" } else { "icmp slt" },
                ValueType::Bool,
            ),
            BinaryOp::LessEqual => (
                if is_float { "fcmp ole" } else { "icmp sle" },
                ValueType::Bool,
            ),
            BinaryOp::Greater => (
                if is_float { "fcmp ogt" } else { "icmp sgt" },
                ValueType::Bool,
            ),
            BinaryOp::GreaterEqual => (
                if is_float { "fcmp oge" } else { "icmp sge" },
                ValueType::Bool,
            ),
            BinaryOp::LogicalAnd | BinaryOp::LogicalOr => unreachable!(),
        };
        self.instruction(format!(
            "{temp} = {instruction} {} {}, {}",
            llvm_value_type(left_type),
            left.repr,
            right.repr
        ));
        Value {
            ty: result_type.into(),
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
        self.current_block = label.to_owned();
        self.terminated = false;
    }
}

fn llvm_value_type(ty: ValueType) -> &'static str {
    match ty {
        ValueType::I32 => "i32",
        ValueType::F32 => "float",
        ValueType::Bool => "i1",
    }
}

fn llvm_return_type(ty: ReturnType) -> &'static str {
    match ty {
        ReturnType::Value(ty) => llvm_value_type(ty),
        ReturnType::Void => "void",
    }
}

fn llvm_constant(value: ConstantValue) -> String {
    match value {
        ConstantValue::I32(value) => value.to_string(),
        ConstantValue::F32(value) => llvm_float(value),
        ConstantValue::Bool(value) => value.to_string(),
    }
}

fn llvm_float(value: f32) -> String {
    format!("0x{:016X}", (value as f64).to_bits())
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
draw {
    debug_frame(frames, 1.0)
    clear_rgb(1, 2, 3)
    fill_rect(10, 20, 30, 40, 50, 60, 70)
}
"#;
        let ir = compile_to_llvm(source).expect("compilation should pass");
        assert!(ir.contains("define i32 @spk_fn_add"));
        assert!(ir.contains("call i32 @spk_fn_add"));
        assert!(ir.contains("icmp sgt i32"));
        assert!(ir.contains("define void @spk_update(float %arg0)"));
        assert!(ir.contains("call void @crumb_clear_rgb(i32 1, i32 2, i32 3)"));
        assert!(ir.contains(
            "call void @crumb_fill_rect(i32 10, i32 20, i32 30, i32 40, i32 50, i32 60, i32 70)"
        ));

        let tokens = lexer::lex(source).expect("lexing should pass");
        let mut program = parser::parse(tokens).expect("parsing should pass");
        sema::check(&mut program).expect("semantic checking should pass");
    }

    #[test]
    fn emits_conversions_short_circuit_void_constants_and_compound_assignment() {
        let source = r#"game "Features"
const TEN: i32 = 10
let integer: i32 = TEN
let decimal: f32 = 1.5
fn effect() -> void { print_i32(TEN) }
fn yes() -> bool { effect() return true }
start {
    decimal += f32(integer)
    integer *= i32(decimal)
    if false && yes() || true { effect() }
}
update(dt: f32) {}
draw {}
"#;
        let ir = compile_to_llvm(source).expect("compilation should pass");
        assert!(!ir.contains("@spk_global_TEN"));
        assert!(ir.contains("define void @spk_fn_effect"));
        assert!(ir.contains("call void @spk_fn_effect"));
        assert!(ir.contains("ret void"));
        assert!(ir.contains("sitofp i32"));
        assert!(ir.contains("fptosi float"));
        assert!(ir.contains("fptosi_nan"));
        assert!(ir.contains("phi i32"));
        assert!(ir.contains("logical_rhs"));
        assert!(ir.contains("phi i1"));
        assert!(ir.contains("fadd float"));
        assert!(ir.contains("mul i32"));
    }
}
