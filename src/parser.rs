use std::mem::discriminant;

use crate::ast::{
    BinaryOp, Block, Expr, ExprKind, Function, FunctionKind, Global, Param, Program, Stmt,
    StmtKind, Type, UnaryOp,
};
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::{Token, TokenKind};

pub fn parse(tokens: Vec<Token>) -> Result<Program, Vec<Diagnostic>> {
    Parser::new(tokens).run().map_err(|error| vec![error])
}

struct Parser {
    tokens: Vec<Token>,
    cursor: usize,
}

impl Parser {
    fn new(tokens: Vec<Token>) -> Self {
        Self { tokens, cursor: 0 }
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

        let mut globals = Vec::new();
        let mut functions = Vec::new();
        while !self.at(&TokenKind::Eof) {
            if self.at(&TokenKind::Let) {
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
                    "expected a global `let`, `fn`, `start`, `update`, or `draw` declaration",
                ));
            }
        }

        Ok(Program {
            title,
            title_span: title_token.span,
            globals,
            functions,
        })
    }

    fn parse_global(&mut self) -> Result<Global, Diagnostic> {
        let start = self.expect(&TokenKind::Let, "expected `let`")?.span;
        let (name, _) = self.identifier("expected a global variable name")?;
        self.expect(&TokenKind::Colon, "expected `:` after variable name")?;
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Equal, "expected `=` before global initializer")?;
        let init = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(init.span);
        Ok(Global {
            name,
            ty,
            init,
            span: start.merge(end),
        })
    }

    fn parse_start(&mut self) -> Result<Function, Diagnostic> {
        let start = self.expect(&TokenKind::Start, "expected `start`")?.span;
        let (body, end) = self.block()?;
        Ok(Function {
            name: "start".into(),
            kind: FunctionKind::Start,
            params: Vec::new(),
            return_type: Type::Void,
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
        let ty = self.parse_type()?;
        self.expect(
            &TokenKind::RightParen,
            "expected `)` after the frame-delta parameter",
        )?;
        let (body, end) = self.block()?;
        Ok(Function {
            name: "update".into(),
            kind: FunctionKind::Update,
            params: vec![Param {
                name,
                ty,
                span: name_span,
            }],
            return_type: Type::Void,
            body,
            span: start.merge(end),
        })
    }

    fn parse_draw(&mut self) -> Result<Function, Diagnostic> {
        let start = self.expect(&TokenKind::Draw, "expected `draw`")?.span;
        let (body, end) = self.block()?;
        Ok(Function {
            name: "draw".into(),
            kind: FunctionKind::Draw,
            params: Vec::new(),
            return_type: Type::Void,
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
                let ty = self.parse_type()?;
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
        let return_type = self.parse_type()?;
        let (body, end) = self.block()?;
        Ok(Function {
            name,
            kind: FunctionKind::Named,
            params,
            return_type,
            body,
            span: start.merge(end),
        })
    }

    fn parse_type(&mut self) -> Result<Type, Diagnostic> {
        let token = self.advance();
        match token.kind {
            TokenKind::I32 => Ok(Type::I32),
            TokenKind::F32 => Ok(Type::F32),
            TokenKind::Bool => Ok(Type::Bool),
            _ => Err(Diagnostic::new(
                "expected type `i32`, `f32`, or `bool`",
                token.span,
            )),
        }
    }

    fn block(&mut self) -> Result<(Block, Span), Diagnostic> {
        self.expect(&TokenKind::LeftBrace, "expected `{` to begin block")?;
        let mut statements = Vec::new();
        while !self.at(&TokenKind::RightBrace) && !self.at(&TokenKind::Eof) {
            statements.push(self.statement()?);
        }
        let end = self
            .expect(&TokenKind::RightBrace, "expected `}` to close block")?
            .span;
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
        if matches!(self.current().kind, TokenKind::Identifier(_))
            && self.peek_is(&TokenKind::Equal)
        {
            return self.assignment_statement();
        }

        let expression = self.expression()?;
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
        let ty = self.parse_type()?;
        self.expect(&TokenKind::Equal, "expected `=` before initializer")?;
        let init = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(init.span);
        Ok(Stmt {
            kind: StmtKind::Let { name, ty, init },
            span: start.merge(end),
        })
    }

    fn assignment_statement(&mut self) -> Result<Stmt, Diagnostic> {
        let (name, start) = self.identifier("expected a variable name")?;
        self.expect(&TokenKind::Equal, "expected `=` in assignment")?;
        let value = self.expression()?;
        let end = self.optional_semicolon().unwrap_or(value.span);
        Ok(Stmt {
            kind: StmtKind::Assign { name, value },
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
        self.equality()
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
            self.primary()
        }
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
            TokenKind::Identifier(name) => {
                if !self.take(&TokenKind::LeftParen) {
                    return Ok(Expr {
                        kind: ExprKind::Variable(name),
                        span: token.span,
                    });
                }
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
                Ok(Expr {
                    kind: ExprKind::Call { name, args },
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

    fn peek_is(&self, kind: &TokenKind) -> bool {
        self.tokens
            .get(self.cursor + 1)
            .is_some_and(|token| discriminant(&token.kind) == discriminant(kind))
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
    fn reports_missing_block_close() {
        let tokens = lexer::lex("game \"Oops\"\nstart {").expect("lexing should pass");
        let errors = parse(tokens).expect_err("parsing should fail");
        assert!(errors[0].message.contains("expected `}`"));
    }
}
