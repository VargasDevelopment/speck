use std::collections::{HashMap, HashSet};

use crate::ast::{StructDecl, ValueType};
use crate::diagnostic::Diagnostic;

pub(super) struct StructTypeAnalysis {
    invalid_lengths: HashSet<String>,
}

impl StructTypeAnalysis {
    pub(super) fn new(
        structs: &HashMap<String, StructDecl>,
        diagnostics: &mut Vec<Diagnostic>,
    ) -> Self {
        let mut declarations: Vec<_> = structs.values().collect();
        declarations.sort_by_key(|declaration| (declaration.span.start, &declaration.name));
        let indices: HashMap<_, _> = declarations
            .iter()
            .enumerate()
            .map(|(index, declaration)| (declaration.name.as_str(), index))
            .collect();
        let mut dependencies = vec![Vec::new(); declarations.len()];
        let mut dependents = vec![Vec::new(); declarations.len()];
        let mut invalid = vec![false; declarations.len()];
        for (index, declaration) in declarations.iter().enumerate() {
            for field in &declaration.fields {
                invalid[index] |= field.ty.has_invalid_array_length();
                if let Some(dependency) = struct_name(&field.ty).and_then(|name| indices.get(name))
                {
                    dependencies[index].push(*dependency);
                    dependents[*dependency].push(index);
                }
            }
        }

        // Strongly connected components identify exactly the structs containing
        // themselves, without blaming acyclic wrappers around a recursive type.
        let recursive = recursive_members(&dependencies, &dependents);
        for (index, declaration) in declarations.iter().enumerate() {
            if recursive[index] {
                diagnostics.push(Diagnostic::new(
                    format!(
                        "recursive value type is not supported: struct `{}` contains itself",
                        declaration.name
                    ),
                    declaration.span,
                ));
            }
        }

        // Propagate failed array lengths once across the same graph. Per-value
        // validation must not expand shared dependencies back into a tree.
        let mut pending: Vec<_> = (0..invalid.len()).filter(|&index| invalid[index]).collect();
        while let Some(index) = pending.pop() {
            for &dependent in &dependents[index] {
                if !invalid[dependent] {
                    invalid[dependent] = true;
                    pending.push(dependent);
                }
            }
        }
        Self {
            invalid_lengths: declarations
                .iter()
                .enumerate()
                .filter(|(index, _)| invalid[*index])
                .map(|(_, declaration)| declaration.name.clone())
                .collect(),
        }
    }

    pub(super) fn resolution_failed(&self, ty: &ValueType) -> bool {
        ty.has_invalid_array_length()
            || struct_name(ty).is_some_and(|name| self.invalid_lengths.contains(name))
    }
}

fn struct_name(mut ty: &ValueType) -> Option<&str> {
    while let ValueType::Array { element, .. } = ty {
        ty = element;
    }
    match ty {
        ValueType::Struct(name) => Some(name),
        _ => None,
    }
}

fn recursive_members(dependencies: &[Vec<usize>], dependents: &[Vec<usize>]) -> Vec<bool> {
    // Kosaraju's two passes visit each vertex and edge a bounded number of times.
    // Explicit DFS frames also keep long declaration chains off the call stack.
    let mut visited = vec![false; dependencies.len()];
    let mut finished = Vec::with_capacity(dependencies.len());
    let mut stack = Vec::new();
    for root in 0..dependencies.len() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        stack.push((root, 0));
        while let Some((node, next_edge)) = stack.last_mut() {
            if let Some(&dependency) = dependencies[*node].get(*next_edge) {
                *next_edge += 1;
                if !visited[dependency] {
                    visited[dependency] = true;
                    stack.push((dependency, 0));
                }
            } else {
                finished.push(*node);
                stack.pop();
            }
        }
    }

    visited.fill(false);
    let mut recursive = vec![false; dependencies.len()];
    let mut pending = Vec::new();
    let mut component = Vec::new();
    for root in finished.into_iter().rev() {
        if visited[root] {
            continue;
        }
        visited[root] = true;
        pending.push(root);
        component.clear();
        while let Some(node) = pending.pop() {
            component.push(node);
            for &dependent in &dependents[node] {
                if !visited[dependent] {
                    visited[dependent] = true;
                    pending.push(dependent);
                }
            }
        }
        if component.len() > 1 || dependencies[root].contains(&root) {
            for &node in &component {
                recursive[node] = true;
            }
        }
    }
    recursive
}
