use crate::ast::{Expr, ExprKind};
use crate::diagnostic::{Diagnostic, Span};

// Keep recursive parsing and later AST traversal/destruction within ordinary
// host thread stacks. This is a syntax/tree limit, not a declaration-graph limit.
pub(super) const MAX_NESTING: usize = 80;

pub(super) fn exceeded(span: Span) -> Diagnostic {
    Diagnostic::new(
        format!("source nesting exceeds limit of {MAX_NESTING}"),
        span,
    )
}

pub(super) fn check_expression(expression: &Expr, nesting: usize) -> Result<(), Diagnostic> {
    // Check every newly constructed expression, including loop-built binary and
    // postfix chains. Children have already passed this check, so even a rejected
    // parent is only one level deeper and can be safely dropped on the error path.
    let mut pending = vec![(expression, nesting)];
    while let Some((expression, depth)) = pending.pop() {
        if depth >= MAX_NESTING {
            return Err(exceeded(expression.span));
        }
        let depth = depth + 1;
        match &expression.kind {
            ExprKind::Unary { operand, .. } => pending.push((operand, depth)),
            ExprKind::Field { base, .. } => pending.push((base, depth)),
            ExprKind::Binary { left, right, .. } => {
                pending.push((left, depth));
                pending.push((right, depth));
            }
            ExprKind::Index { base, index } => {
                pending.push((base, depth));
                pending.push((index, depth));
            }
            ExprKind::ArrayLiteral(elements)
            | ExprKind::Call { args: elements, .. }
            | ExprKind::Conversion { args: elements, .. } => {
                pending.extend(elements.iter().map(|element| (element, depth)));
            }
            ExprKind::StructLiteral { fields, .. } => {
                pending.extend(fields.iter().map(|field| (&field.value, depth)));
            }
            ExprKind::I32(_) | ExprKind::F32(_) | ExprKind::Bool(_) | ExprKind::Variable(_) => {}
        }
    }
    Ok(())
}
