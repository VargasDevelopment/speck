use std::collections::HashMap;
use std::fmt::Write;

use crate::ast::{
    ArrayLength, AssignOp, BinaryOp, Block, ConstantValue, Expr, ExprKind, Function, FunctionKind,
    Program, ReturnType, Stmt, StmtKind, StructDecl, UnaryOp, ValueType,
};
use crate::builtins;

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
            .clone()
    }
}

pub fn emit(program: &Program) -> String {
    emit_for_target(program, None)
}

pub fn emit_for_target(program: &Program, target_triple: Option<&str>) -> String {
    let functions = function_signatures(program);
    let structs = program
        .structs
        .iter()
        .map(|declaration| (declaration.name.clone(), declaration.clone()))
        .collect::<HashMap<_, _>>();
    let mut globals: HashMap<_, _> = program
        .globals
        .iter()
        .map(|global| {
            (
                global.name.clone(),
                Variable {
                    ty: global.ty.clone(),
                    pointer: format!("@spk_global_{}", global.name),
                },
            )
        })
        .collect();
    let mut constants = builtins::CONSTANTS
        .iter()
        .map(|constant| (constant.name.to_owned(), constant.value.clone()))
        .collect::<HashMap<_, _>>();
    constants.extend(program.constants.iter().map(|constant| {
        (
            constant.name.clone(),
            constant
                .value
                .clone()
                .expect("semantic checking evaluates every constant"),
        )
    }));
    for constant in &program.constants {
        let value = constant
            .value
            .as_ref()
            .expect("semantic checking evaluates every constant");
        if is_aggregate_constant(value) {
            globals.insert(
                constant.name.clone(),
                Variable {
                    ty: constant.ty.clone(),
                    pointer: format!("@spk_const_{}", constant.name),
                },
            );
        }
    }

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
    for builtin in builtins::FUNCTIONS {
        let params = builtin
            .params
            .iter()
            .map(llvm_value_type)
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            output,
            "declare {} {}({params})",
            llvm_return_type(&builtin.return_type),
            builtin.llvm_symbol
        )
        .expect("writing to a string cannot fail");
    }
    output.push_str("declare void @crumb_bounds_fail(i32, i32)\n");
    output.push('\n');

    for declaration in &program.structs {
        let fields = declaration
            .fields
            .iter()
            .map(|field| llvm_value_type(&field.ty))
            .collect::<Vec<_>>()
            .join(", ");
        writeln!(
            output,
            "%spk_struct_{} = type {{ {fields} }}",
            declaration.name
        )
        .expect("writing to a string cannot fail");
    }
    if !program.structs.is_empty() {
        output.push('\n');
    }

    for constant in &program.constants {
        let value = constant
            .value
            .as_ref()
            .expect("semantic checking evaluates every constant");
        if is_aggregate_constant(value) {
            writeln!(
                output,
                "@spk_const_{} = internal constant {} {}",
                constant.name,
                llvm_value_type(&constant.ty),
                llvm_constant(value)
            )
            .expect("writing to a string cannot fail");
        }
    }
    for global in &program.globals {
        let value = global
            .value
            .as_ref()
            .expect("semantic checking evaluates every global initializer");
        writeln!(
            output,
            "@spk_global_{} = internal global {} {}",
            global.name,
            llvm_value_type(&global.ty),
            llvm_constant(value)
        )
        .expect("writing to a string cannot fail");
    }
    if !program.globals.is_empty()
        || program
            .constants
            .iter()
            .any(|constant| constant.value.as_ref().is_some_and(is_aggregate_constant))
    {
        output.push('\n');
    }

    for function in &program.functions {
        let emitter = FunctionEmitter::new(function, &globals, &constants, &structs, &functions);
        output.push_str(&emitter.emit());
        output.push('\n');
    }
    if output.ends_with("\n\n") {
        output.pop();
    }
    output
}

fn function_signatures(program: &Program) -> HashMap<String, Signature> {
    let mut signatures = builtins::FUNCTIONS
        .iter()
        .map(|builtin| {
            (
                builtin.name.to_owned(),
                Signature {
                    return_type: builtin.return_type.clone(),
                    symbol: builtin.llvm_symbol.to_owned(),
                },
            )
        })
        .collect::<HashMap<_, _>>();
    for function in &program.functions {
        if function.kind == FunctionKind::Named {
            signatures.insert(
                function.name.clone(),
                Signature {
                    return_type: function.return_type.clone(),
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
    structs: &'a HashMap<String, StructDecl>,
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
        structs: &'a HashMap<String, StructDecl>,
        functions: &'a HashMap<String, Signature>,
    ) -> Self {
        Self {
            function,
            globals,
            constants,
            structs,
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
            .map(|(index, param)| format!("{} %arg{index}", llvm_value_type(&param.ty)))
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
            llvm_return_type(&self.function.return_type)
        ));
        self.lines.push("entry:".into());

        for (index, param) in self.function.params.iter().enumerate() {
            let pointer = self.temp();
            self.instruction(format!("{pointer} = alloca {}", llvm_value_type(&param.ty)));
            self.instruction(format!(
                "store {} %arg{index}, ptr {pointer}",
                llvm_value_type(&param.ty)
            ));
            self.scopes[0].insert(
                param.name.clone(),
                Variable {
                    ty: param.ty.clone(),
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
                let value = self.expression_as(init, ty);
                let pointer = self.temp();
                self.instruction(format!("{pointer} = alloca {}", llvm_value_type(ty)));
                self.instruction(format!(
                    "store {} {}, ptr {pointer}",
                    llvm_value_type(ty),
                    value.repr
                ));
                self.scopes
                    .last_mut()
                    .expect("a function always has a local scope")
                    .insert(
                        name.clone(),
                        Variable {
                            ty: ty.clone(),
                            pointer,
                        },
                    );
            }
            StmtKind::Assign { target, op, value } => {
                let variable = self
                    .lvalue(target)
                    .expect("semantic checking guarantees assignment targets");
                let value = if *op == AssignOp::Set {
                    self.expression_as(value, &variable.ty)
                } else {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = load {}, ptr {}",
                        llvm_value_type(&variable.ty),
                        variable.pointer
                    ));
                    let left = Value {
                        ty: variable.ty.clone().into(),
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
                    llvm_value_type(&variable.ty),
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
                        llvm_value_type(&value.value_type()),
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

    fn expression_as(&mut self, expression: &Expr, expected: &ValueType) -> Value {
        let ExprKind::ArrayLiteral(elements) = &expression.kind else {
            return self.expression(expression);
        };
        let (element_type, _) = expected
            .resolved_array()
            .expect("semantic checking matches array literals to array types");
        let aggregate_type = llvm_value_type(expected);
        let element_llvm_type = llvm_value_type(element_type);
        let mut aggregate = "undef".to_owned();
        for (index, element) in elements.iter().enumerate() {
            let value = self.expression_as(element, element_type);
            let temp = self.temp();
            self.instruction(format!(
                "{temp} = insertvalue {aggregate_type} {aggregate}, {element_llvm_type} {}, {index}",
                value.repr
            ));
            aggregate = temp;
        }
        Value {
            ty: expected.clone().into(),
            repr: aggregate,
        }
    }

    fn struct_literal(
        &mut self,
        name: &str,
        initializers: &[crate::ast::FieldInitializer],
    ) -> Value {
        let declaration = self
            .structs
            .get(name)
            .expect("semantic checking guarantees struct literal types")
            .clone();
        let struct_type = ValueType::Struct(name.to_owned());
        let aggregate_type = llvm_value_type(&struct_type);
        let mut aggregate = "undef".to_owned();
        for (index, field) in declaration.fields.iter().enumerate() {
            let initializer = initializers
                .iter()
                .find(|initializer| initializer.name == field.name)
                .expect("semantic checking guarantees every field initializer");
            let value = self.expression_as(&initializer.value, &field.ty);
            let temp = self.temp();
            self.instruction(format!(
                "{temp} = insertvalue {aggregate_type} {aggregate}, {} {}, {index}",
                llvm_value_type(&field.ty),
                value.repr
            ));
            aggregate = temp;
        }
        Value {
            ty: struct_type.into(),
            repr: aggregate,
        }
    }

    fn lvalue(&mut self, expression: &Expr) -> Option<Variable> {
        match &expression.kind {
            ExprKind::Variable(name) => self.variable(name),
            ExprKind::Index { base, index } => {
                let base = self.lvalue(base)?;
                let (element_type, length) = base.ty.resolved_array()?;
                let element_type = element_type.clone();
                let array_type = llvm_value_type(&base.ty);
                let index = self.expression(index);
                self.bounds_check(&index.repr, length);
                let pointer = self.temp();
                self.instruction(format!(
                    "{pointer} = getelementptr inbounds {array_type}, ptr {}, i32 0, i32 {}",
                    base.pointer, index.repr
                ));
                Some(Variable {
                    ty: element_type,
                    pointer,
                })
            }
            ExprKind::Field { base, name, .. } => {
                let base = self.lvalue(base)?;
                let ValueType::Struct(struct_name) = &base.ty else {
                    return None;
                };
                let declaration = self.structs.get(struct_name)?;
                let (index, field) = declaration
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, field)| field.name == name.as_str())?;
                let field_type = field.ty.clone();
                let aggregate_type = llvm_value_type(&base.ty);
                let pointer = self.temp();
                self.instruction(format!(
                    "{pointer} = getelementptr inbounds {aggregate_type}, ptr {}, i32 0, i32 {index}",
                    base.pointer
                ));
                Some(Variable {
                    ty: field_type,
                    pointer,
                })
            }
            _ => None,
        }
    }

    fn bounds_check(&mut self, index: &str, length: usize) {
        let nonnegative = self.temp();
        self.instruction(format!("{nonnegative} = icmp sge i32 {index}, 0"));
        let below_length = self.temp();
        self.instruction(format!("{below_length} = icmp slt i32 {index}, {length}"));
        let valid = self.temp();
        self.instruction(format!("{valid} = and i1 {nonnegative}, {below_length}"));
        let valid_label = self.label("bounds_valid");
        let failure_label = self.label("bounds_failure");
        self.terminate(format!(
            "br i1 {valid}, label %{valid_label}, label %{failure_label}"
        ));

        self.place_label(&failure_label);
        self.instruction(format!(
            "call void @crumb_bounds_fail(i32 {index}, i32 {length})"
        ));
        self.terminate("unreachable".into());

        self.place_label(&valid_label);
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
            ExprKind::ArrayLiteral(_) => {
                unreachable!("array literals are emitted with their declared type")
            }
            ExprKind::StructLiteral { name, fields } => self.struct_literal(name, fields),
            ExprKind::Variable(name) => {
                if let Some(variable) = self.variable(name) {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = load {}, ptr {}",
                        llvm_value_type(&variable.ty),
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
                        .cloned()
                        .expect("semantic checking guarantees constant references");
                    Value {
                        ty: value.ty().into(),
                        repr: llvm_constant(&value),
                    }
                }
            }
            ExprKind::Index { base, index } => {
                let variable = if let Some(variable) = self.lvalue(expression) {
                    variable
                } else {
                    let base = self.expression(base);
                    let base_type = base.value_type();
                    let (element_type, length) = base_type
                        .resolved_array()
                        .expect("semantic checking guarantees array index bases");
                    let element_type = element_type.clone();
                    let array_type = llvm_value_type(&base_type);
                    let storage = self.entry_alloca(&array_type);
                    self.instruction(format!("store {array_type} {}, ptr {storage}", base.repr));
                    let index = self.expression(index);
                    self.bounds_check(&index.repr, length);
                    let pointer = self.temp();
                    self.instruction(format!(
                        "{pointer} = getelementptr inbounds {array_type}, ptr {storage}, i32 0, i32 {}",
                        index.repr
                    ));
                    Variable {
                        ty: element_type,
                        pointer,
                    }
                };
                let temp = self.temp();
                self.instruction(format!(
                    "{temp} = load {}, ptr {}",
                    llvm_value_type(&variable.ty),
                    variable.pointer
                ));
                Value {
                    ty: variable.ty.into(),
                    repr: temp,
                }
            }
            ExprKind::Field { base, name, .. } => {
                if let Some(variable) = self.lvalue(expression) {
                    let temp = self.temp();
                    self.instruction(format!(
                        "{temp} = load {}, ptr {}",
                        llvm_value_type(&variable.ty),
                        variable.pointer
                    ));
                    return Value {
                        ty: variable.ty.into(),
                        repr: temp,
                    };
                }
                let base = self.expression(base);
                let ValueType::Struct(struct_name) = base.value_type() else {
                    unreachable!("semantic checking guarantees struct field bases");
                };
                let declaration = self
                    .structs
                    .get(&struct_name)
                    .expect("semantic checking guarantees struct types");
                let (index, field) = declaration
                    .fields
                    .iter()
                    .enumerate()
                    .find(|(_, field)| field.name == name.as_str())
                    .expect("semantic checking guarantees field names");
                let field_type = field.ty.clone();
                let temp = self.temp();
                self.instruction(format!(
                    "{temp} = extractvalue %spk_struct_{struct_name} {}, {index}",
                    base.repr
                ));
                Value {
                    ty: field_type.into(),
                    repr: temp,
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
                        format!("{} {}", llvm_value_type(&value.value_type()), value.repr)
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
                        llvm_return_type(&signature.return_type),
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
                match (source_type, target.clone()) {
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
            BinaryOp::Add => (if is_float { "fadd" } else { "add" }, left_type.clone()),
            BinaryOp::Subtract => (if is_float { "fsub" } else { "sub" }, left_type.clone()),
            BinaryOp::Multiply => (if is_float { "fmul" } else { "mul" }, left_type.clone()),
            BinaryOp::Divide => (if is_float { "fdiv" } else { "sdiv" }, left_type.clone()),
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
            llvm_value_type(&left_type),
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

    fn entry_alloca(&mut self, llvm_type: &str) -> String {
        let pointer = self.temp();
        self.lines
            .insert(2, format!("  {pointer} = alloca {llvm_type}"));
        pointer
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

fn llvm_value_type(ty: &ValueType) -> String {
    match ty {
        ValueType::I32 => "i32".into(),
        ValueType::F32 => "float".into(),
        ValueType::Bool => "i1".into(),
        ValueType::Struct(name) => format!("%spk_struct_{name}"),
        ValueType::Array {
            element,
            length: ArrayLength::Resolved(length),
        } => format!("[{length} x {}]", llvm_value_type(element)),
        ValueType::Array { .. } => {
            unreachable!("semantic checking resolves every array length before code generation")
        }
    }
}

fn llvm_return_type(ty: &ReturnType) -> String {
    match ty {
        ReturnType::Value(ty) => llvm_value_type(ty),
        ReturnType::Void => "void".into(),
    }
}

fn llvm_constant(value: &ConstantValue) -> String {
    match value {
        ConstantValue::I32(value) => value.to_string(),
        ConstantValue::F32(value) => llvm_float(*value),
        ConstantValue::Bool(value) => value.to_string(),
        ConstantValue::Struct { fields, .. } => {
            let fields = fields
                .iter()
                .map(|(_, value)| {
                    format!("{} {}", llvm_value_type(&value.ty()), llvm_constant(value))
                })
                .collect::<Vec<_>>()
                .join(", ");
            format!("{{ {fields} }}")
        }
        ConstantValue::Array {
            element_type,
            elements,
        } => {
            let element_type = llvm_value_type(element_type);
            let elements = elements
                .iter()
                .map(|value| format!("{element_type} {}", llvm_constant(value)))
                .collect::<Vec<_>>()
                .join(", ");
            format!("[{elements}]")
        }
    }
}

fn is_aggregate_constant(value: &ConstantValue) -> bool {
    matches!(
        value,
        ConstantValue::Array { .. } | ConstantValue::Struct { .. }
    )
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

    #[test]
    fn emits_input_abi_calls_and_inlines_predefined_keys() {
        let source = r#"game "Input IR"
let held: bool = false
start {}
update(dt: f32) {
    held = key_down(KEY_LEFT)
    if key_pressed(KEY_SPACE) || key_released(KEY_ENTER) { quit() }
}
draw {}
"#;
        let ir = compile_to_llvm(source).expect("input program should compile");
        assert!(ir.contains("declare i1 @crumb_key_down(i32)"));
        assert!(ir.contains("declare i1 @crumb_key_pressed(i32)"));
        assert!(ir.contains("declare i1 @crumb_key_released(i32)"));
        assert!(ir.contains("declare void @crumb_request_quit()"));
        assert!(ir.contains("call i1 @crumb_key_down(i32 6)"));
        assert!(ir.contains("call i1 @crumb_key_pressed(i32 8)"));
        assert!(ir.contains("call i1 @crumb_key_released(i32 9)"));
        assert!(ir.contains("call void @crumb_request_quit()"));
        assert!(!ir.contains("@spk_global_KEY_"));
    }
}
