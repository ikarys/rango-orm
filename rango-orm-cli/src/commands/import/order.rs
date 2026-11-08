//! Foreign-key ordering of tables.

use std::collections::HashSet;

/// Orders tables so that referenced (parent) tables come before the tables that
/// reference them. `edges` are `(child, parent)` pairs; edges touching tables not
/// in `tables`, and self-references, are ignored. Ties are broken alphabetically so
/// the order is deterministic.
pub(crate) fn order_by_dependencies(
    mut tables: Vec<String>,
    edges: &[(String, String)],
) -> Vec<String> {
    tables.sort();
    tables.dedup();

    let table_set: HashSet<&str> = tables.iter().map(String::as_str).collect();

    let mut indegree: std::collections::HashMap<String, usize> =
        tables.iter().map(|t| (t.clone(), 0)).collect();
    let mut children: std::collections::HashMap<String, Vec<String>> =
        tables.iter().map(|t| (t.clone(), Vec::new())).collect();

    for (child, parent) in edges {
        if child == parent {
            continue;
        }
        if table_set.contains(child.as_str()) && table_set.contains(parent.as_str()) {
            if let Some(dependents) = children.get_mut(parent) {
                dependents.push(child.clone());
            }
            if let Some(deg) = indegree.get_mut(child) {
                *deg += 1;
            }
        }
    }

    let mut available: Vec<String> = tables
        .iter()
        .filter(|t| indegree.get(*t).copied().unwrap_or(0) == 0)
        .cloned()
        .collect();
    available.sort();

    let mut ordered = Vec::with_capacity(tables.len());
    while !available.is_empty() {
        let table = available.remove(0);
        ordered.push(table.clone());

        if let Some(dependents) = children.get(&table) {
            for dependent in dependents {
                if let Some(deg) = indegree.get_mut(dependent) {
                    *deg -= 1;
                    if *deg == 0 {
                        available.push(dependent.clone());
                    }
                }
            }
        }

        available.sort();
    }

    // Cycles between tables are inserted in alphabetical order; on Postgres the nullable FK
    // columns that point forward are filled in afterwards (see `import_postgres`).
    let ordered_set: HashSet<String> = ordered.iter().cloned().collect();
    let mut remaining: Vec<String> = tables
        .iter()
        .filter(|t| !ordered_set.contains(*t))
        .cloned()
        .collect();
    remaining.sort();
    ordered.extend(remaining);

    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_by_dependencies_puts_parents_first() {
        let tables = vec![
            "post".to_string(),
            "comment".to_string(),
            "user".to_string(),
        ];
        let edges = vec![
            ("post".to_string(), "user".to_string()),
            ("comment".to_string(), "post".to_string()),
            ("comment".to_string(), "user".to_string()),
        ];
        assert_eq!(
            order_by_dependencies(tables, &edges),
            vec![
                "user".to_string(),
                "post".to_string(),
                "comment".to_string()
            ]
        );
    }

    #[test]
    fn order_by_dependencies_ignores_self_refs_and_unknown_tables() {
        let tables = vec!["b".to_string(), "a".to_string()];
        let edges = vec![
            ("a".to_string(), "a".to_string()),
            ("b".to_string(), "zzz".to_string()),
        ];
        assert_eq!(
            order_by_dependencies(tables, &edges),
            vec!["a".to_string(), "b".to_string()]
        );
    }

    #[test]
    fn order_by_dependencies_keeps_cycles() {
        let tables = vec!["x".to_string(), "y".to_string(), "z".to_string()];
        let edges = vec![
            ("x".to_string(), "y".to_string()),
            ("y".to_string(), "x".to_string()),
            ("x".to_string(), "z".to_string()),
        ];
        let ordered = order_by_dependencies(tables, &edges);
        assert_eq!(ordered[0], "z");
        assert_eq!(ordered.len(), 3);
        assert!(ordered.contains(&"x".to_string()));
        assert!(ordered.contains(&"y".to_string()));
    }
}
