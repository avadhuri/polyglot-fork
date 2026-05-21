//! AC-4.4 position smoke tests — verify `Expression::Hole(N)` is admitted at
//! every AST position ADR-0037 Consequences names.
//!
//! Background: ADR-0037 §0037.3 replaces the per-hole `polyglot_parse_at`
//! approach with a single composition entry point `parse_recipe_select_text()`
//! that parses the full sentinel-form SQL statement and then pattern-matches the
//! resulting AST.  The Hole sentinel `«hole:N»` must therefore be accepted by
//! the parser at the structural position it occupies in the SQL string.
//!
//! These tests parse complete SQL statements with `«hole:N»` sentinels and
//! verify that `Expression::Hole(N)` appears at the STRUCTURAL AST position
//! named by ADR-0037 Consequences (not merely somewhere in the tree).
//!
//! ADR-0037 Consequences (line 138):
//!   "polyglot-fork's parse_at.rs rule table covers column_reference,
//!    table_reference, boolean_expression, comparison, expression,
//!    select_list_item"
//!
//! AC-4.2 (cargo test regression gate) is satisfied by all tests in this file
//! passing alongside the existing hole_smoke_test.rs suite.

use polyglot_sql::dialects::Dialect;
use polyglot_sql::expressions::{Expression, From, Select, Where};

// ─── helpers ──────────────────────────────────────────────────────────────────

fn pg() -> Dialect {
    Dialect::get_by_name("postgres").expect("postgres dialect not registered")
}

/// Parse SQL and return the first (outermost) Select node.
fn parse_select(sql: &str) -> Select {
    let exprs = pg().parse(sql).unwrap_or_else(|e| {
        panic!("parse failed for `{sql}`: {e}");
    });
    for expr in &exprs {
        if let Expression::Select(s) = expr {
            return *s.clone();
        }
    }
    panic!("no Select node in AST for `{sql}`; got: {exprs:?}");
}

// ─── AC-4.4 position: column_reference ───────────────────────────────────────
//
// `column_reference` = a Hole occupies a select-list item position directly.
// ADR-0037 AC-4.4: SELECT «hole:1» FROM _stub_
// Expected structural location: Select::expressions[0] == Hole(1)

#[test]
fn hole_in_column_reference_position() {
    let s = parse_select("SELECT «hole:1» FROM _stub_");
    assert_eq!(
        s.expressions.len(),
        1,
        "expected one expression in SELECT list"
    );
    assert_eq!(
        s.expressions[0],
        Expression::Hole(1),
        "SELECT-list item must be Expression::Hole(1)"
    );
}

// ─── AC-4.4 position: table_reference ────────────────────────────────────────
//
// `table_reference` = a Hole occupies the FROM-clause table-factor position.
// Requires the AC-4.1 fix (parse_table_expression_primary).
// Expected structural location: Select::from.expressions[0] == Hole(2)

#[test]
fn hole_in_table_reference_position() {
    let s = parse_select("SELECT 1 FROM «hole:2»");
    let from: &From = s
        .from
        .as_ref()
        .expect("expected a FROM clause");
    assert_eq!(
        from.expressions.len(),
        1,
        "expected one expression in FROM clause"
    );
    assert_eq!(
        from.expressions[0],
        Expression::Hole(2),
        "FROM-clause expression must be Expression::Hole(2)"
    );
}

// ─── AC-4.4 position: boolean_expression ─────────────────────────────────────
//
// `boolean_expression` = a Hole is the entire WHERE predicate (bare, not
// nested in a comparison).
// Expected structural location: Select::where_clause.this == Hole(3)

#[test]
fn hole_in_boolean_expression_position() {
    let s = parse_select("SELECT 1 FROM _stub_ WHERE «hole:3»");
    let where_clause: &Where = s
        .where_clause
        .as_ref()
        .expect("expected a WHERE clause");
    assert_eq!(
        where_clause.this,
        Expression::Hole(3),
        "WHERE predicate must be Expression::Hole(3)"
    );
}

// ─── AC-4.4 position: comparison ─────────────────────────────────────────────
//
// `comparison` = a Hole is the right-hand side of an equality comparison.
// Expected structural location: Select::where_clause.this is an Eq expression
// whose right operand is Hole(4).

#[test]
fn hole_in_comparison_position() {
    let s = parse_select("SELECT 1 FROM _stub_ WHERE id = «hole:4»");
    let where_clause: &Where = s
        .where_clause
        .as_ref()
        .expect("expected a WHERE clause");

    match &where_clause.this {
        Expression::Eq(op) => {
            assert_eq!(
                op.right,
                Expression::Hole(4),
                "right-hand side of EQ comparison must be Expression::Hole(4)"
            );
        }
        other => panic!(
            "expected WHERE to contain an Eq expression, got: {other:?}"
        ),
    }
}

// ─── AC-4.4 position: comparison — left-hand side ────────────────────────────
//
// Symmetric: a Hole as the LEFT operand of a comparison.

#[test]
fn hole_in_comparison_left_position() {
    let s = parse_select("SELECT 1 FROM _stub_ WHERE «hole:5» = id");
    let where_clause: &Where = s
        .where_clause
        .as_ref()
        .expect("expected a WHERE clause");

    match &where_clause.this {
        Expression::Eq(op) => {
            assert_eq!(
                op.left,
                Expression::Hole(5),
                "left-hand side of EQ comparison must be Expression::Hole(5)"
            );
        }
        other => panic!(
            "expected WHERE to contain an Eq expression, got: {other:?}"
        ),
    }
}

// ─── AC-4.4 position: expression ─────────────────────────────────────────────
//
// `expression` = a Hole appears as a sub-expression inside an arithmetic
// expression in the SELECT list.  Tests that Hole works as an operand of a
// binary operator (not just as a standalone atom).
// Expected: Select::expressions[0] is some binary expression containing Hole(6).

#[test]
fn hole_in_expression_position() {
    let s = parse_select("SELECT «hole:6» + 1 FROM _stub_");
    assert_eq!(s.expressions.len(), 1, "expected one expression in SELECT list");

    // The top-level expression should be a binary op containing Hole(6).
    let top = &s.expressions[0];
    let has_hole = expr_contains_hole_id(top, 6);
    assert!(
        has_hole,
        "arithmetic expression in SELECT list must contain Expression::Hole(6); got: {top:?}"
    );
}

// ─── AC-4.4 position: select_list_item ───────────────────────────────────────
//
// `select_list_item` = a Hole is the expression inside an aliased SELECT-list
// item (`«hole:7» AS col_name`).
// Expected structural location: expressions[0] is Alias { this: Hole(7), alias: "col_name" }

#[test]
fn hole_in_select_list_item_aliased_position() {
    let s = parse_select("SELECT «hole:7» AS col_name FROM _stub_");
    assert_eq!(s.expressions.len(), 1, "expected one expression in SELECT list");

    match &s.expressions[0] {
        Expression::Alias(alias) => {
            assert_eq!(
                alias.this,
                Expression::Hole(7),
                "Alias::this must be Expression::Hole(7)"
            );
            assert_eq!(
                alias.alias.name, "col_name",
                "alias name must be 'col_name'"
            );
        }
        other => panic!(
            "expected Alias expression in SELECT list, got: {other:?}"
        ),
    }
}

// ─── AC-4.4 position: multiple holes in one statement ────────────────────────
//
// The wrapper's role-validation logic depends on being able to identify which
// Hole(N) landed at which Select AST field when multiple holes appear in the
// same statement.  Verify that distinct hole ids remain distinct in the AST.

#[test]
fn multiple_holes_in_one_statement_retain_distinct_ids() {
    let s = parse_select("SELECT «hole:10» FROM «hole:20» WHERE «hole:30»");

    // SELECT list
    assert_eq!(s.expressions[0], Expression::Hole(10), "SELECT-list hole id mismatch");

    // FROM clause
    let from = s.from.as_ref().expect("expected FROM clause");
    assert_eq!(from.expressions[0], Expression::Hole(20), "FROM-clause hole id mismatch");

    // WHERE clause
    let where_clause = s.where_clause.as_ref().expect("expected WHERE clause");
    assert_eq!(where_clause.this, Expression::Hole(30), "WHERE hole id mismatch");
}

// ─── helpers ──────────────────────────────────────────────────────────────────

/// Walk an expression tree looking for a Hole with the given id.
fn expr_contains_hole_id(expr: &Expression, id: u32) -> bool {
    if matches!(expr, Expression::Hole(n) if *n == id) {
        return true;
    }
    use polyglot_sql::traversal::ExpressionWalk;
    expr.dfs().any(|e| matches!(e, Expression::Hole(n) if *n == id))
}
