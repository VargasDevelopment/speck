use std::collections::HashSet;
use std::mem::discriminant;

use crate::ast::{
    ArrayLength, AssignOp, BinaryOp, Block, Constant, Expr, ExprKind, FieldInitializer, Function,
    FunctionKind, Global, Param, Program, ReturnType, Stmt, StmtKind, StructDecl, StructField,
    UnaryOp, ValueType,
};
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: Vec<Token>) -> Result<Program, Vec<Diagnostic>> {
    Parser::new(tokens).run().map_err(|error| vec![error])
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
    struct_names: HashSet<String>,
    value_scopes: Vec<HashSet<String>>,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        let struct_names = tokens
            .windows(2)
            .filter_map(|window| {
                if matches!(window[0].kind, TokenKind::Struct)
                    && let TokenKind::Identifier(name) = &window[1].kind
                {
                    Some(name.clone())
                } else {
                    None
                }
            })
            .collect();
        Self {
            tokens,
            cursor: 0,
            struct_names,
            value_scopes: Vec::new(),
        }
    }

    fn run(mut self) -> Result<Program, Diagnostic> {
        self.expect(&TokenKind::Game, "expected `game` at the start of the file")?;
        let title_token = self.advance();
        let TokenKind::String(title) = title_token.kind else {
            return Err(Diagnostic::new(
                "expected a quoted game title after `game`",
                title_token.span,
            ));
        };
        self.take(&TokenKind::Semicolon);

        let mut structs = Vec::new();
        let mut constants = Vec::new();
        let mut globals = Vec::new();
        let mut functions = Vec::new();
        while !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Struct) {
                structs.push(self.parse_struct()?);
            } else if self.at(&TokenKind::Const) {
                constants.push(self.parse_constant()?);
            } else if self.at(&TokenKind::Let) {
                globals.push(self.parse_global()?);
            } else if self.at(&TokenKind::Start) {
                functions.push(self.parse_start()?);
            } else if self.at(&TokenKind::Update) {
                functions.push(self.parse_update()?);
            } else if self.at(&TokenKind::Draw) {
                functions.push(self.parse_draw()?);
            } else if self.at(&TokenKind::Fn) {
                functions.push(self.parse_named_function()?);
            } else {
                return Err(self.error_here(
                    "expected a top-level `struct`, `const`, `let`, `fn`, `start`, `update`, or `draw` declaration",
                ));
            }
        }

        Ok(Program {
            title,
            title_span: title_token.span,
            structs,
            constants,
            globals,
            functions,
        })
    }

    fn parse_struct(&mut self) -> Result<StructDecl, Diagnostic> {
        let start = self.expect(&TokenKind::Struct, "expected `struct`")?.span;
        let (name, _) = self.identifier("expected a struct name")?;
        self.expect(&TokenKind::LeftBrace, "expected `{` after the struct name")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            let (field_name, field_span) = self.identifier("expected a field name")?;
            self.expect(&TokenKind::Colon, "expected `:` after field name")?;
            let ty = self.parse_value_type()?;
            fields.push(StructField {
                name: field_name,
                ty,
                span: field_span,
            });
            self.take(&TokenKind::Comma);
        }
        let end = self
            .expect(&TokenKind::RightBrace, "expected `}` after struct fields")?
            .span;
        Ok(StructDecl {
            name,
            fields,
            span: start.merge(end),
        })
    }

    fn parse_constant(&mut self) -> Result<Constant, Diagnostic> {
        let start = self.expect(&TokenKind::Const, "expected `const`")?.span;
        let (name, _) = self.identifier("expected a constant name")?;
        self.expect(&TokenKind::Colon, "expected `:` after constant name")?;
        let ty = self.parse_value_type()?;
        self.expect(
            &TokenKind::Equal,
            "expected `=` before constant initializer",
        )?;
        let init = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(init.span);
        Ok(Constant {
            name,
            ty,
            init,
            value: None,
            span: start.merge(end),
        })
    }

    fn parse_global(&mut self) -> Result<Global, Diagnostic> {
        let start = self.expect(&TokenKind::Let, "expected `let`")?.span;
        let (name, _) = self.identifier("expected a global variable name")?;
        self.expect(&TokenKind::Colon, "expected `:` after variable name")?;
        let ty = self.parse_value_type()?;
        self.expect(&TokenKind::Equal, "expected `=` before global initializer")?;
        let init = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(init.span);
        Ok(Global {
            name,
            ty,
            init,
            value: None,
            span: start.merge(end),
        })
    }

    fn parse_start(&mut self) -> Result<Function, Diagnostic> {
        let start = self.expect(&TokenKind::Start, "expected `start`")?.span;
        let (body, end) = self.function_block(Vec::new())?;
        Ok(Function {
            name: "start".into(),
            kind: FunctionKind::Start,
            params: Vec::new(),
            return_type: ReturnType::Void,
            body,
            span: start.merge(end),
        })
    }

    fn parse_update(&mut self) -> Result<Function, Diagnostic> {
        let start = self.expect(&TokenKind::Update, "expected `update`")?.span;
        self.expect(&TokenKind::LeftParen, "expected `(` after `update`")?;
        let (name, name_span) = self.identifier("expected the frame-delta parameter name")?;
        self.expect(
            &TokenKind::Colon,
            "expected `:` after the frame-delta parameter name",
        )?;
        let ty = self.parse_value_type()?;
        self.expect(
            &TokenKind::RightParen,
            "expected `)` after the frame-delta parameter",
        )?;
        let (body, end) = self.function_block(vec![name.clone()])?;
        Ok(Function {
            name: "update".into(),
            kind: FunctionKind::Update,
            params: vec![Param {
                name,
                ty,
                span: name_span,
            }],
            return_type: ReturnType::Void,
            body,
            span: start.merge(end),
        })
    }

    fn parse_draw(&mut self) -> Result<Function, Diagnostic> {
        let start = self.expect(&TokenKind::Draw, "expected `draw`")?.span;
        let (body, end) = self.function_block(Vec::new())?;
        Ok(Function {
            name: "draw".into(),
            kind: FunctionKind::Draw,
            params: Vec::new(),
            return_type: ReturnType::Void,
            body,
            span: start.merge(end),
        })
    }

    fn parse_named_function(&mut self) -> Result<Function, Diagnostic> {
        let start = self.expect(&TokenKind::Fn, "expected `fn`")?.span;
        let (name, _) = self.identifier("expected a function name")?;
        self.expect(&TokenKind::LeftParen, "expected `(` after function name")?;
        let mut params = Vec::new();
        if !self.at(&TokenKind::RightParen) {
            loop {
                let (param_name, span) = self.identifier("expected a parameter name")?;
                self.expect(&TokenKind::Colon, "expected `:` after parameter name")?;
                let ty = self.parse_value_type()?;
                params.push(Param {
                    name: param_name,
                    ty,
                    span,
                });
                if !self.take(&TokenKind::Comma) {
                    break;
                }
            }
        }
        self.expect(&TokenKind::RightParen, "expected `)` after parameters")?;
        self.expect(
            &TokenKind::Arrow,
            "expected `->` and a return type after parameters",
        )?;
        let return_type = self.parse_return_type()?;
        let parameter_names = params.iter().map(|param| param.name.clone()).collect();
        let (body, end) = self.function_block(parameter_names)?;
        Ok(Function {
            name,
            kind: FunctionKind::Named,
            params,
            return_type,
            body,
            span: start.merge(end),
        })
    }

    fn parse_value_type(&mut self) -> Result<ValueType, Diagnostic> {
        let token = self.advance();
        match token.kind {
            TokenKind::I32 => Ok(ValueType::I32),
            TokenKind::F32 => Ok(ValueType::F32),
            TokenKind::Bool => Ok(ValueType::Bool),
            TokenKind::Identifier(name) => Ok(ValueType::Struct(name)),
            TokenKind::LeftBracket => {
                let element = self.parse_value_type()?;
                self.expect(
                    &TokenKind::Semicolon,
                    "expected `;` between the array element type and length",
                )?;
                let length = if self.take(&TokenKind::Minus) {
                    let token = self.advance();
                    let TokenKind::Integer(value) = token.kind else {
                        return Err(Diagnostic::new(
                            "expected an integer literal after `-` in array length",
                            token.span,
                        ));
                    };
                    ArrayLength::Literal {
                        value: -value,
                        span: token.span,
                    }
                } else {
                    let token = self.advance();
                    match token.kind {
                        TokenKind::Integer(value) => ArrayLength::Literal {
                            value,
                            span: token.span,
                        },
                        TokenKind::Identifier(name) => ArrayLength::Constant {
                            name,
                            span: token.span,
                        },
                        _ => {
                            return Err(Diagnostic::new(
                                "array length must be a positive integer literal or an `i32` constant",
                                token.span,
                            ));
                        }
                    }
                };
                self.expect(&TokenKind::RightBracket, "expected `]` after array type")?;
                Ok(ValueType::Array {
                    element: Box::new(element),
                    length,
                })
            }
            TokenKind::Void => Err(Diagnostic::new(
                "`void` is only valid as a function return type",
                token.span,
            )),
            _ => Err(Diagnostic::new(
                "expected a scalar, struct, or fixed array type",
                token.span,
            )),
        }
    }

    fn parse_return_type(&mut self) -> Result<ReturnType, Diagnostic> {
        if self.take(&TokenKind::Void) {
            Ok(ReturnType::Void)
        } else {
            self.parse_value_type().map(ReturnType::Value)
        }
    }

    fn function_block(&mut self, bindings: Vec<String>) -> Result<(Block, Span), Diagnostic> {
        self.value_scopes.push(bindings.into_iter().collect());
        let result = self.block();
        self.value_scopes.pop();
        result
    }

    fn block(&mut self) -> Result<(Block, Span), Diagnostic> {
        self.expect(&TokenKind::LeftBrace, "expected `{` to begin block")?;
        self.value_scopes.push(HashSet::new());
        let mut statements = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        let end = self
            .expect(&TokenKind::RightBrace, "expected `}` to close block")?
            .span;
        self.value_scopes.pop();
        Ok((statements, end))
    }

    fn statement(&mut self) -> Result<Stmt, Diagnostic> {
        if self.at(&TokenKind::Let) {
            return self.let_statement();
        }
        if self.at(&TokenKind::If) {
            return self.if_statement();
        }
        if self.at(&TokenKind::While) {
            return self.while_statement();
        }
        if self.at(&TokenKind::Return) {
            return self.return_statement();
        }
        let expression = self.expression()?;
        if self.at_assignment_operator() {
            return self.assignment_statement(expression);
        }
        let end = self.optional_semicolon().unwrap_or(expression.span);
        let span = expression.span.merge(end);
        Ok(Stmt {
            kind: StmtKind::Expr(expression),
            span,
        })
    }

    fn let_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect(&TokenKind::Let, "expected `let`")?.span;
        let (name, _) = self.identifier("expected a local variable name")?;
        self.expect(&TokenKind::Colon, "expected `:` after variable name")?;
        let ty = self.parse_value_type()?;
        self.expect(&TokenKind::Equal, "expected `=` before initializer")?;
        let init = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(init.span);
        self.value_scopes
            .last_mut()
            .expect("local declarations are parsed inside a block")
            .insert(name.clone());
        Ok(Stmt {
            kind: StmtKind::Let { name, ty, init },
            span: start.merge(end),
        })
    }

    fn assignment_statement(&mut self, target: Expr) -> Result<Stmt, Diagnostic> {
        let start = target.span;
        let token = self.advance();
        let op = match token.kind {
            TokenKind::Equal => AssignOp::Set,
            TokenKind::PlusEqual => AssignOp::Add,
            TokenKind::MinusEqual => AssignOp::Subtract,
            TokenKind::StarEqual => AssignOp::Multiply,
            TokenKind::SlashEqual => AssignOp::Divide,
            _ => {
                return Err(Diagnostic::new(
                    "expected an assignment operator",
                    token.span,
                ));
            }
        };
        let value = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(value.span);
        Ok(Stmt {
            kind: StmtKind::Assign { target, op, value },
            span: start.merge(end),
        })
    }

    fn if_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect(&TokenKind::If, "expected `if`")?.span;
        let condition = self.expression()?;
        let (then_block, then_end) = self.block()?;
        let (else_block, end) = if self.take(&TokenKind::Else) {
            let (block, block_end) = self.block()?;
            (Some(block), block_end)
        } else {
            (None, then_end)
        };
        Ok(Stmt {
            kind: StmtKind::If {
                condition,
                then_block,
                else_block,
            },
            span: start.merge(end),
        })
    }

    fn while_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect(&TokenKind::While, "expected `while`")?.span;
        let condition = self.expression()?;
        let (body, end) = self.block()?;
        Ok(Stmt {
            kind: StmtKind::While { condition, body },
            span: start.merge(end),
        })
    }

    fn return_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let start = self.expect(&TokenKind::Return, "expected `return`")?.span;
        let value = if self.at(&TokenKind::RightBrace) || self.at(&TokenKind::Semicolon) {
            None
        } else {
            Some(self.expression()?)
        };
        let value_end = value.as_ref().map(|expression| expression.span);
        let end = self.optional_semicolon().or(value_end).unwrap_or(start);
        Ok(Stmt {
            kind: StmtKind::Return(value),
            span: start.merge(end),
        })
    }

    fn expression(&mut self) -> Result<Expr, Diagnostic> {
        self.logical_or()
    }

    fn logical_or(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.logical_and()?;
        while self.take(&TokenKind::OrOr) {
            let right = self.logical_and()?;
            expression = binary(expression, BinaryOp::LogicalOr, right);
        }
        Ok(expression)
    }

    fn logical_and(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.equality()?;
        while self.take(&TokenKind::AndAnd) {
            let right = self.equality()?;
            expression = binary(expression, BinaryOp::LogicalAnd, right);
        }
        Ok(expression)
    }

    fn equality(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.comparison()?;
        loop {
            let op = if self.take(&TokenKind::EqualEqual) {
                Some(BinaryOp::Equal)
            } else if self.take(&TokenKind::BangEqual) {
                Some(BinaryOp::NotEqual)
            } else {
                None
            };
            let Some(op) = op else { break };
            let right = self.comparison()?;
            expression = binary(expression, op, right);
        }
        Ok(expression)
    }

    fn comparison(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.term()?;
        loop {
            let op = if self.take(&TokenKind::Less) {
                Some(BinaryOp::Less)
            } else if self.take(&TokenKind::LessEqual) {
                Some(BinaryOp::LessEqual)
            } else if self.take(&TokenKind::Greater) {
                Some(BinaryOp::Greater)
            } else if self.take(&TokenKind::GreaterEqual) {
                Some(BinaryOp::GreaterEqual)
            } else {
                None
            };
            let Some(op) = op else { break };
            let right = self.term()?;
            expression = binary(expression, op, right);
        }
        Ok(expression)
    }

    fn term(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.factor()?;
        loop {
            let op = if self.take(&TokenKind::Plus) {
                Some(BinaryOp::Add)
            } else if self.take(&TokenKind::Minus) {
                Some(BinaryOp::Subtract)
            } else {
                None
            };
            let Some(op) = op else { break };
            let right = self.factor()?;
            expression = binary(expression, op, right);
        }
        Ok(expression)
    }

    fn factor(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.unary()?;
        loop {
            let op = if self.take(&TokenKind::Star) {
                Some(BinaryOp::Multiply)
            } else if self.take(&TokenKind::Slash) {
                Some(BinaryOp::Divide)
            } else {
                None
            };
            let Some(op) = op else { break };
            let right = self.unary()?;
            expression = binary(expression, op, right);
        }
        Ok(expression)
    }

    fn unary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.current().clone();
        let op = if self.take(&TokenKind::Minus) {
            Some(UnaryOp::Negate)
        } else if self.take(&TokenKind::Bang) {
            Some(UnaryOp::Not)
        } else {
            None
        };
        if let Some(op) = op {
            let operand = self.unary()?;
            let span = token.span.merge(operand.span);
            Ok(Expr {
                kind: ExprKind::Unary {
                    op,
                    operand: Box::new(operand),
                },
                span,
            })
        } else {
            self.postfix()
        }
    }

    fn postfix(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.primary()?;
        loop {
            if self.take(&TokenKind::LeftBracket) {
                let index = self.expression()?;
                let end = self
                    .expect(&TokenKind::RightBracket, "expected `]` after array index")?
                    .span;
                let span = expression.span.merge(end);
                expression = Expr {
                    kind: ExprKind::Index {
                        base: Box::new(expression),
                        index: Box::new(index),
                    },
                    span,
                };
            } else if self.take(&TokenKind::Dot) {
                let (name, name_span) = self.identifier("expected a field name after `.`")?;
                let span = expression.span.merge(name_span);
                expression = Expr {
                    kind: ExprKind::Field {
                        base: Box::new(expression),
                        name,
                        name_span,
                    },
                    span,
                };
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.advance();
        match token.kind {
            TokenKind::Integer(value) => Ok(Expr {
                kind: ExprKind::I32(value),
                span: token.span,
            }),
            TokenKind::Float(value) => Ok(Expr {
                kind: ExprKind::F32(value),
                span: token.span,
            }),
            TokenKind::True | TokenKind::False => Ok(Expr {
                kind: ExprKind::Bool(matches!(token.kind, TokenKind::True)),
                span: token.span,
            }),
            TokenKind::LeftBracket => {
                let mut elements = Vec::new();
                if !self.at(&TokenKind::RightBracket) {
                    loop {
                        elements.push(self.expression()?);
                        if !self.take(&TokenKind::Comma) {
                            break;
                        }
                        if self.at(&TokenKind::RightBracket) {
                            break;
                        }
                    }
                }
                let end = self
                    .expect(&TokenKind::RightBracket, "expected `]` after array literal")?
                    .span;
                Ok(Expr {
                    kind: ExprKind::ArrayLiteral(elements),
                    span: token.span.merge(end),
                })
            }
            TokenKind::Identifier(name) => {
                if self.looks_like_struct_literal(&name) {
                    return self.struct_literal(name, token.span);
                }
                if !self.take(&TokenKind::LeftParen) {
                    return Ok(Expr {
                        kind: ExprKind::Variable(name),
                        span: token.span,
                    });
                }
                let (args, end) = self.arguments()?;
                Ok(Expr {
                    kind: ExprKind::Call { name, args },
                    span: token.span.merge(end),
                })
            }
            TokenKind::I32 | TokenKind::F32 => {
                let target = if matches!(token.kind, TokenKind::I32) {
                    ValueType::I32
                } else {
                    ValueType::F32
                };
                self.expect(
                    &TokenKind::LeftParen,
                    "expected `(` after numeric conversion type",
                )?;
                let (args, end) = self.arguments()?;
                Ok(Expr {
                    kind: ExprKind::Conversion { target, args },
                    span: token.span.merge(end),
                })
            }
            TokenKind::LeftParen => {
                let mut expression = self.expression()?;
                let end = self
                    .expect(&TokenKind::RightParen, "expected `)` after expression")?
                    .span;
                expression.span = token.span.merge(end);
                Ok(expression)
            }
            _ => Err(Diagnostic::new("expected an expression", token.span)),
        }
    }

    fn struct_literal(&mut self, name: String, start: Span) -> Result<Expr, Diagnostic> {
        self.expect(&TokenKind::LeftBrace, "expected `{` after struct type name")?;
        let mut fields = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            let (field_name, field_start) = self.identifier("expected a field initializer name")?;
            self.expect(
                &TokenKind::Colon,
                "expected `:` after field initializer name",
            )?;
            let value = self.expression()?;
            let span = field_start.merge(value.span);
            fields.push(FieldInitializer {
                name: field_name,
                value,
                span,
            });
            self.take(&TokenKind::Comma);
        }
        let end = self
            .expect(&TokenKind::RightBrace, "expected `}` after struct literal")?
            .span;
        Ok(Expr {
            kind: ExprKind::StructLiteral { name, fields },
            span: start.merge(end),
        })
    }

    fn looks_like_struct_literal(&self, name: &str) -> bool {
        if !self.at(&TokenKind::LeftBrace) || self.is_value_binding(name) {
            return false;
        }
        self.struct_names.contains(name)
            || matches!(
                (
                    self.tokens.get(self.cursor + 1).map(|token| &token.kind),
                    self.tokens.get(self.cursor + 2).map(|token| &token.kind)
                ),
                (Some(TokenKind::Identifier(_)), Some(TokenKind::Colon))
            )
    }

    fn is_value_binding(&self, name: &str) -> bool {
        self.value_scopes
            .iter()
            .rev()
            .any(|scope| scope.contains(name))
    }

    fn arguments(&mut self) -> Result<(Vec<Expr>, Span), Diagnostic> {
        let mut args = Vec::new();
        if !self.at(&TokenKind::RightParen) {
            loop {
                args.push(self.expression()?);
                if !self.take(&TokenKind::Comma) {
                    break;
                }
            }
        }
        let end = self
            .expect(&TokenKind::RightParen, "expected `)` after arguments")?
            .span;
        Ok((args, end))
    }

    fn identifier(&mut self, message: &str) -> Result<(String, Span), Diagnostic> {
        let token = self.advance();
        if let TokenKind::Identifier(name) = token.kind {
            Ok((name, token.span))
        } else {
            Err(Diagnostic::new(message, token.span))
        }
    }

    fn optional_semicolon(&mut self) -> Option<Span> {
        if self.at(&TokenKind::Semicolon) {
            Some(self.advance().span)
        } else {
            None
        }
    }

    fn expect(&mut self, kind: &TokenKind, message: &str) -> Result<Token, Diagnostic> {
        if self.at(kind) {
            Ok(self.advance())
        } else {
            Err(self.error_here(message))
        }
    }

    fn take(&mut self, kind: &TokenKind) -> bool {
        if self.at(kind) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn at(&self, kind: &TokenKind) -> bool {
        discriminant(&self.current().kind) == discriminant(kind)
    }

    fn at_assignment_operator(&self) -> bool {
        matches!(
            self.current().kind,
            TokenKind::Equal
                | TokenKind::PlusEqual
                | TokenKind::MinusEqual
                | TokenKind::StarEqual
                | TokenKind::SlashEqual
        )
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor]
    }

    fn advance(&mut self) -> Token {
        let token = self.current().clone();
        if !matches!(token.kind, TokenKind::Eof) {
            self.cursor += 1;
        }
        token
    }

    fn error_here(&self, message: &str) -> Diagnostic {
        Diagnostic::new(message, self.current().span)
    }
}

fn binary(left: Expr, op: BinaryOp, right: Expr) -> Expr {
    let span = left.span.merge(right.span);
    Expr {
        kind: ExprKind::Binary {
            left: Box::new(left),
            op,
            right: Box::new(right),
        },
        span,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lexer;

    fn parse_source(source: &str) -> Program {
        parse(lexer::lex(source).expect("lexing should pass")).expect("parsing should pass")
    }

    #[test]
    fn multiplication_binds_more_tightly_than_addition() {
        let program = parse_source(
            "game \"Precedence\"\nlet result: i32 = 1 + 2 * 3\nstart {}\nupdate(dt: f32) {}\ndraw {}",
        );
        let ExprKind::Binary { op, right, .. } = &program.globals[0].init.kind else {
            panic!("expected a binary expression");
        };
        assert_eq!(*op, BinaryOp::Add);
        assert!(matches!(
            right.kind,
            ExprKind::Binary {
                op: BinaryOp::Multiply,
                ..
            }
        ));
    }

    #[test]
    fn parses_functions_control_flow_and_calls() {
        let program = parse_source(
            r#"game "Flow"
fn choose(value: i32) -> i32 {
    if value > 0 { return value } else { return 0 }
}
start { let x: i32 = 2; while x > 0 { x = x - 1 } print_i32(choose(x)) }
update(dt: f32) {}
draw {}
"#,
        );
        assert_eq!(program.functions.len(), 4);
        assert_eq!(program.functions[0].name, "choose");
        assert!(matches!(
            program.functions[0].body[0].kind,
            StmtKind::If { .. }
        ));
    }

    #[test]
    fn parses_array_types_literals_indexing_and_indexed_assignment() {
        let program = parse_source(
            r#"game "Arrays"
const COUNT: i32 = 3
let values: [i32; COUNT] = [1, 2, 3]
start { let value: i32 = values[1] values[2] += value }
update(dt: f32) {}
draw {}
"#,
        );
        assert!(matches!(
            program.globals[0].ty,
            ValueType::Array {
                length: ArrayLength::Constant { .. },
                ..
            }
        ));
        assert!(matches!(
            program.globals[0].init.kind,
            ExprKind::ArrayLiteral(_)
        ));
        assert!(matches!(
            program.functions[0].body[1].kind,
            StmtKind::Assign {
                target: Expr {
                    kind: ExprKind::Index { .. },
                    ..
                },
                op: AssignOp::Add,
                ..
            }
        ));
    }

    #[test]
    fn parses_struct_declarations_literals_fields_and_field_assignment() {
        let program = parse_source(
            r#"game "Structs"
struct Point { x: i32, y: i32 }
let point: Point = Point { y: 2, x: 1 }
start { let x: i32 = point.x point.y += x }
update(dt: f32) {}
draw {}
"#,
        );
        assert_eq!(program.structs[0].name, "Point");
        assert_eq!(program.structs[0].fields.len(), 2);
        assert!(matches!(
            program.globals[0].init.kind,
            ExprKind::StructLiteral { .. }
        ));
        assert!(matches!(
            program.functions[0].body[1].kind,
            StmtKind::Assign {
                target: Expr {
                    kind: ExprKind::Field { .. },
                    ..
                },
                op: AssignOp::Add,
                ..
            }
        ));
    }

    #[test]
    fn reports_missing_block_close() {
        let tokens = lexer::lex("game \"Oops\"\nstart {").expect("lexing should pass");
        let errors = parse(tokens).expect_err("parsing should fail");
        assert!(errors[0].message.contains("expected `}`"));
    }

    #[test]
    fn parses_constants_void_conversions_and_compound_assignments() {
        let program = parse_source(
            r#"game "Syntax"
const LIMIT: i32 = 10
let x: f32 = f32(LIMIT)
fn effect() -> void { return }
start { x += 1.0 x -= 1.0 x *= 2.0 x /= 2.0 effect() }
update(dt: f32) {}
draw {}
"#,
        );
        assert_eq!(program.constants.len(), 1);
        assert_eq!(program.functions[0].return_type, ReturnType::Void);
        assert!(matches!(
            program.globals[0].init.kind,
            ExprKind::Conversion {
                target: ValueType::F32,
                ..
            }
        ));
        let operators = program.functions[1]
            .body
            .iter()
            .filter_map(|statement| match statement.kind {
                StmtKind::Assign { op, .. } => Some(op),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            operators,
            vec![
                AssignOp::Add,
                AssignOp::Subtract,
                AssignOp::Multiply,
                AssignOp::Divide,
            ]
        );
    }

    #[test]
    fn boolean_composition_has_the_documented_precedence() {
        let program = parse_source(
            "game \"Booleans\"\nconst VALUE: bool = false || true && 1 == 1\nstart {}\nupdate(dt: f32) {}\ndraw {}",
        );
        let ExprKind::Binary { op, right, .. } = &program.constants[0].init.kind else {
            panic!("expected logical-or expression");
        };
        assert_eq!(*op, BinaryOp::LogicalOr);
        let ExprKind::Binary {
            op: right_op,
            right: equality,
            ..
        } = &right.kind
        else {
            panic!("expected logical-and expression");
        };
        assert_eq!(*right_op, BinaryOp::LogicalAnd);
        assert!(matches!(
            equality.kind,
            ExprKind::Binary {
                op: BinaryOp::Equal,
                ..
            }
        ));
    }

    #[test]
    fn rejects_void_in_value_type_positions() {
        for source in [
            "game \"Bad\"\nlet value: void = 1\nstart {}\nupdate(dt: f32) {}\ndraw {}",
            "game \"Bad\"\nconst VALUE: void = 1\nstart {}\nupdate(dt: f32) {}\ndraw {}",
            "game \"Bad\"\nfn bad(value: void) -> void {}\nstart {}\nupdate(dt: f32) {}\ndraw {}",
            "game \"Bad\"\nstart { let value: void = 1 }\nupdate(dt: f32) {}\ndraw {}",
        ] {
            let tokens = lexer::lex(source).expect("lexing should pass");
            let errors = parse(tokens).expect_err("void value type should fail");
            assert!(
                errors[0]
                    .message
                    .contains("only valid as a function return type")
            );
        }
    }
}
