// parse_at.rs — AvaDhuri extension to polyglot-sql FFI
//
// Adds polyglot_parse_at(sql_fragment, rule, dialect) → PolyglotResult
//
// parse_at validates a SQL fragment against a named grammatical role
// (e.g., "column_expr", "where_clause").  The upstream polyglot-sql FFI
// does not expose this entry point; this is a local extension.
//
// Implementation strategy: synthetic-statement wrapping.
//   - "column_expr":   SELECT {fragment} FROM _stub_
//   - "where_clause":  SELECT 1 FROM _stub_ WHERE {fragment}
//   - "expr":          SELECT {fragment} FROM _stub_
//   - "select_item":   SELECT {fragment} FROM _stub_
//   - fallback:        attempt to parse the fragment as a full statement
//
// The wrapping approach avoids needing to expose sqlglot's internal
// rule-entry points through the C ABI.  If the wrapped statement parses
// cleanly, the fragment is valid in that role.
//
// Limitations (acceptable for POC scope):
//   - Error spans are relative to the synthetic wrapper, not the fragment.
//     The error message is adjusted to name the rule explicitly.
//   - Rules not in the explicit map fall through to full-statement parse.
//
// Per POC-001 ticket: parse_at availability was an open question (OQ-2).
// This extension resolves it by forking rather than waiting for upstream.

use crate::helpers::{dialect_by_name, err_result, ok_json_result, panic_result, required_arg};
use crate::types::{PolyglotResult, STATUS_PARSE_ERROR, STATUS_INVALID_ARGUMENT};
use std::os::raw::c_char;

/// Validate a SQL fragment against a named grammatical role.
///
/// # Arguments
/// - `sql_fragment`: the fragment to validate (e.g., `user_id + 1`)
/// - `rule`:         the grammatical role (e.g., `column_expr`, `where_clause`)
/// - `dialect`:      the SQL dialect (e.g., `postgres`, `snowflake`)
///
/// # Returns
/// `PolyglotResult` with status 0 on success.  `data` contains a minimal
/// JSON object `{"ok":true,"rule":"<rule>"}`.  On failure, `error` contains
/// a human-readable message and `status` is STATUS_PARSE_ERROR.
///
/// Caller must free the result with `polyglot_free_result`.
#[no_mangle]
pub extern "C" fn polyglot_parse_at(
    sql_fragment: *const c_char,
    rule: *const c_char,
    dialect: *const c_char,
) -> PolyglotResult {
    match std::panic::catch_unwind(|| parse_at_impl(sql_fragment, rule, dialect)) {
        Ok(result) => result,
        Err(panic) => panic_result(panic),
    }
}

fn parse_at_impl(
    sql_fragment: *const c_char,
    rule: *const c_char,
    dialect: *const c_char,
) -> PolyglotResult {
    let fragment = match unsafe { required_arg(sql_fragment, "sql_fragment") } {
        Ok(v) => v,
        Err(r) => return r,
    };
    let rule_name = match unsafe { required_arg(rule, "rule") } {
        Ok(v) => v,
        Err(r) => return r,
    };
    let dialect_name = match unsafe { required_arg(dialect, "dialect") } {
        Ok(v) => v,
        Err(r) => return r,
    };

    let dialect_impl = match dialect_by_name(&dialect_name) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Sanitization: sub-expression rules cannot legitimately contain statement
    // terminators. Without this guard, an injection-style input like
    // `; DROP TABLE users; --` would wrap into `SELECT ; DROP TABLE users; -- FROM _stub_`,
    // which sqlparser-rs parses as two statements (empty SELECT + valid DROP).
    // The wrapped parse succeeds, and parse_at incorrectly reports the fragment
    // as valid in the named role. Sub-expression rules must not contain `;`.
    let sub_expression_rules = [
        "column_expr", "expr", "select_expr", "select_item", "projection",
        "where_clause", "predicate", "condition", "bool_expr", "filter",
        "order_by_expr", "order_expr", "sort_expr",
        "group_by_expr", "group_expr",
        "having_expr", "having",
        "table_ref", "from_item", "relation",
    ];
    if sub_expression_rules.contains(&rule_name.as_str()) && fragment.contains(';') {
        return err_result(
            STATUS_PARSE_ERROR,
            format!(
                "fragment is not valid as `{}` in dialect `{}`: contains statement terminator (`;`)",
                rule_name, dialect_name
            ),
        );
    }

    // Build a synthetic SQL statement that embeds the fragment in the
    // position corresponding to the named rule.
    let synthetic_sql = build_synthetic_statement(&fragment, &rule_name);

    match dialect_impl.parse(&synthetic_sql) {
        Ok(_) => {
            // Parse succeeded — fragment is valid in the named role.
            let ok_payload = serde_json::json!({
                "ok": true,
                "rule": rule_name,
                "dialect": dialect_name,
                "fragment": fragment,
            });
            ok_json_result(&ok_payload)
        }
        Err(e) => {
            // Parse failed — fragment is invalid in the named role.
            let msg = format!(
                "fragment is not valid as `{}` in dialect `{}`: {}",
                rule_name, dialect_name, e
            );
            err_result(STATUS_PARSE_ERROR, msg)
        }
    }
}

/// Map a grammatical role name to a synthetic SQL statement that places
/// the fragment in the correct syntactic position.
///
/// The placeholder table name `_stub_` is used where a table reference is
/// needed.  It is syntactically valid in all supported dialects.
fn build_synthetic_statement(fragment: &str, rule: &str) -> String {
    match rule {
        // Column expression: appears in the SELECT list
        "column_expr" | "expr" | "select_expr" | "select_item" | "projection" => {
            format!("SELECT {} FROM _stub_", fragment)
        }
        // WHERE clause predicate
        "where_clause" | "predicate" | "condition" | "bool_expr" | "filter" => {
            format!("SELECT 1 FROM _stub_ WHERE {}", fragment)
        }
        // ORDER BY expression
        "order_by_expr" | "order_expr" | "sort_expr" => {
            format!("SELECT 1 FROM _stub_ ORDER BY {}", fragment)
        }
        // GROUP BY expression
        "group_by_expr" | "group_expr" => {
            format!("SELECT 1 FROM _stub_ GROUP BY {}", fragment)
        }
        // HAVING clause
        "having_expr" | "having" => {
            format!("SELECT 1 FROM _stub_ GROUP BY 1 HAVING {}", fragment)
        }
        // Table reference / FROM clause item
        "table_ref" | "from_item" | "relation" => {
            format!("SELECT 1 FROM {}", fragment)
        }
        // Fallback: attempt to parse the fragment as a full statement.
        // This handles "statement", "query", and any unknown rule names.
        _ => fragment.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn column_expr_wraps_correctly() {
        let sql = build_synthetic_statement("user_id + 1", "column_expr");
        assert_eq!(sql, "SELECT user_id + 1 FROM _stub_");
    }

    #[test]
    fn where_clause_wraps_correctly() {
        let sql = build_synthetic_statement("age > 18 AND active = true", "where_clause");
        assert_eq!(sql, "SELECT 1 FROM _stub_ WHERE age > 18 AND active = true");
    }

    #[test]
    fn unknown_rule_uses_fragment_as_statement() {
        let sql = build_synthetic_statement("SELECT 1", "statement");
        assert_eq!(sql, "SELECT 1");
    }
}
