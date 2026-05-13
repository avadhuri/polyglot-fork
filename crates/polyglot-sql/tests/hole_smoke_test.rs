//! Smoke test for the AvaDhuri typed-hole extension (POC-002).
//!
//! Verifies that:
//!   1. The lexer recognizes «hole:N» as TokenType::Hole.
//!   2. The parser accepts Hole as a primary atom.
//!   3. The resulting AST contains Expression::Hole { id } nodes.
//!
//! This is Group B (hole acceptance in supported positions) — the foundational
//! tests for the parse_with_holes pipeline. Full Group B coverage of every
//! (AST kind, role) pair lives in the parse_with_holes FFI crate's tests.

use polyglot_sql::dialects::Dialect;
use polyglot_sql::expressions::Expression;
use polyglot_sql::tokens::{Tokenizer, TokenType};

#[test]
fn lexer_recognizes_hole_sentinel() {
    let tokenizer = Tokenizer::default();
    let tokens = tokenizer.tokenize("SELECT «hole:42» FROM users").unwrap();

    // Tokens should be: SELECT, Hole(42), FROM, users
    let hole_token = tokens
        .iter()
        .find(|t| t.token_type == TokenType::Hole)
        .expect("expected a Hole token in the stream");
    assert_eq!(hole_token.text, "42");
}

#[test]
fn lexer_handles_multi_digit_hole_id() {
    let tokenizer = Tokenizer::default();
    let tokens = tokenizer.tokenize("SELECT «hole:12345»").unwrap();
    let hole_token = tokens
        .iter()
        .find(|t| t.token_type == TokenType::Hole)
        .expect("expected a Hole token");
    assert_eq!(hole_token.text, "12345");
}

#[test]
fn parser_accepts_hole_as_column_expression() {
    let dialect = Dialect::get_by_name("postgres").expect("postgres dialect not registered");
    let result = dialect.parse("SELECT «hole:7» FROM users").expect("parse failed");

    // The first expression should be a Select containing a Hole in its projection.
    // We don't dig too deep into structure here — just confirm a Hole exists somewhere.
    let any_hole = contains_hole(&result);
    assert!(any_hole, "expected an Expression::Hole node in the AST");
}

#[test]
fn parser_accepts_hole_in_where_predicate_position() {
    let dialect = Dialect::get_by_name("postgres").expect("postgres dialect not registered");
    // Use a binary expression with a hole on one side so the parser sees Hole
    // through parse_primary as an atom of an Expr.
    let result = dialect
        .parse("SELECT * FROM users WHERE id = «hole:3»")
        .expect("parse failed");
    let any_hole = contains_hole(&result);
    assert!(any_hole, "expected an Expression::Hole node in the AST");
}

#[test]
fn lexer_rejects_malformed_hole_missing_close() {
    let tokenizer = Tokenizer::default();
    let result = tokenizer.tokenize("SELECT «hole:42 FROM users");
    assert!(
        result.is_err(),
        "expected tokenization error for missing » closer"
    );
}

#[test]
fn lexer_rejects_malformed_hole_no_digits() {
    let tokenizer = Tokenizer::default();
    let result = tokenizer.tokenize("SELECT «hole:» FROM users");
    assert!(
        result.is_err(),
        "expected tokenization error for missing decimal digits"
    );
}

#[test]
fn lexer_rejects_malformed_hole_wrong_prefix() {
    let tokenizer = Tokenizer::default();
    let result = tokenizer.tokenize("SELECT «foo:42» FROM users");
    assert!(
        result.is_err(),
        "expected tokenization error for wrong 'hole' prefix"
    );
}

// ── Group D: sentinel-collision corpus ────────────────────────────────
// Verify that legitimate user identifiers and string literals which
// "look like" the sentinel pattern do NOT tokenize as Hole tokens.
// The non-ASCII guillemets «» in our sentinel make accidental collision
// structurally impossible — these tests lock that property in place.

#[test]
fn ascii_underscore_hole_identifier_is_not_a_hole() {
    let tokenizer = Tokenizer::default();
    let tokens = tokenizer.tokenize("SELECT hole_1, _HOLE_, HOLE, __hole_42__ FROM users").unwrap();
    let any_hole = tokens.iter().any(|t| t.token_type == TokenType::Hole);
    assert!(
        !any_hole,
        "ASCII identifiers that 'look like' hole sentinels must NOT tokenize as Hole tokens"
    );
}

#[test]
fn quoted_string_containing_sentinel_chars_is_not_a_hole() {
    // String literals must be opaque to the lexer's hole recognizer.
    // A string literal containing the «hole:N» characters is just a string.
    let tokenizer = Tokenizer::default();
    let tokens = tokenizer.tokenize("SELECT 'this looks like «hole:5» but is a string' FROM users").unwrap();
    let any_hole = tokens.iter().any(|t| t.token_type == TokenType::Hole);
    assert!(
        !any_hole,
        "string literals containing the sentinel chars must not produce Hole tokens"
    );
}

#[test]
fn legitimate_double_angle_quote_punctuation_isnt_a_hole() {
    // The « character without the full «hole:N» pattern should produce
    // a tokenization error (not silently accept). This proves that random
    // « in input doesn't get misinterpreted as a half-formed hole.
    let tokenizer = Tokenizer::default();
    let result = tokenizer.tokenize("SELECT « FROM users");
    assert!(
        result.is_err(),
        "a bare « without the full hole sentinel pattern must error, not silently accept"
    );
}

/// Recursively walk the AST looking for any Expression::Hole node.
fn contains_hole(exprs: &[Expression]) -> bool {
    exprs.iter().any(expr_contains_hole)
}

fn expr_contains_hole(expr: &Expression) -> bool {
    if matches!(expr, Expression::Hole(_)) {
        return true;
    }
    // Use the built-in DFS iterator from the ExpressionWalk trait
    use polyglot_sql::traversal::ExpressionWalk;
    expr.dfs().any(|e| matches!(e, Expression::Hole(_)))
}
