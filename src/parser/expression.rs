use super::{Parser, limits};
use crate::ast::{BinaryOp, Expr, ExprKind, FieldInitializer, UnaryOp, ValueType};
use crate::diagnostic::{Diagnostic, Span};
use crate::lexer::TokenKind;

impl Parser {
    pub(super) fn expression(&mut self) -> Result<Expr, Diagnostic> {
        self.nested(Self::logical_or)
    }

    fn logical_or(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.logical_and()?;
        while self.take(&TokenKind::OrOr) {
            let right = self.logical_and()?;
            expression = self.binary(expression, BinaryOp::LogicalOr, right)?;
        }
        Ok(expression)
    }

    fn logical_and(&mut self) -> Result<Expr, Diagnostic> {
        let mut expression = self.equality()?;
        while self.take(&TokenKind::AndAnd) {
            let right = self.equality()?;
            expression = self.binary(expression, BinaryOp::LogicalAnd, right)?;
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
            expression = self.binary(expression, op, right)?;
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
            expression = self.binary(expression, op, right)?;
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
            expression = self.binary(expression, op, right)?;
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
            } else if self.take(&TokenKind::Percent) {
                Some(BinaryOp::Remainder)
            } else {
                None
            };
            let Some(op) = op else { break };
            let right = self.unary()?;
            expression = self.binary(expression, op, right)?;
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
            let operand = self.nested(Self::unary)?;
            let span = token.span.merge(operand.span);
            self.checked(Expr {
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
                expression = self.checked(Expr {
                    kind: ExprKind::Index {
                        base: Box::new(expression),
                        index: Box::new(index),
                    },
                    span,
                })?;
            } else if self.take(&TokenKind::Dot) {
                let (name, name_span) = self.identifier("expected a field name after `.`")?;
                let span = expression.span.merge(name_span);
                expression = self.checked(Expr {
                    kind: ExprKind::Field {
                        base: Box::new(expression),
                        name,
                        name_span,
                    },
                    span,
                })?;
            } else {
                break;
            }
        }
        Ok(expression)
    }

    fn primary(&mut self) -> Result<Expr, Diagnostic> {
        let token = self.advance();
        match token.kind {
            TokenKind::Integer(value) => self.checked(Expr {
                kind: ExprKind::I32(value),
                span: token.span,
            }),
            TokenKind::Float(value) => self.checked(Expr {
                kind: ExprKind::F32(value),
                span: token.span,
            }),
            TokenKind::True | TokenKind::False => self.checked(Expr {
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
                self.checked(Expr {
                    kind: ExprKind::ArrayLiteral(elements),
                    span: token.span.merge(end),
                })
            }
            TokenKind::Identifier(name) => {
                if self.looks_like_struct_literal(&name) {
                    return self.struct_literal(name, token.span);
                }
                if !self.take(&TokenKind::LeftParen) {
                    return self.checked(Expr {
                        kind: ExprKind::Variable(name),
                        span: token.span,
                    });
                }
                let (args, end) = self.arguments()?;
                self.checked(Expr {
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
                self.checked(Expr {
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
        self.checked(Expr {
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

    fn checked(&self, expression: Expr) -> Result<Expr, Diagnostic> {
        limits::check_expression(&expression, self.nesting)?;
        Ok(expression)
    }

    fn binary(&self, left: Expr, op: BinaryOp, right: Expr) -> Result<Expr, Diagnostic> {
        let span = left.span.merge(right.span);
        self.checked(Expr {
            kind: ExprKind::Binary {
                left: Box::new(left),
                op,
                right: Box::new(right),
            },
            span,
        })
    }
}
