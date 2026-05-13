// parse_with_holes.rs — FFI entry point for typed-template hole validation.
//
// polyglot_parse_with_holes(sql_with_sentinels, hole_role_map_json, dialect)
//   → PolyglotResult
//
// Pipeline:
//   1. Parse the SQL via the dialect's parser (lexer recognizes «hole:N»,
//      parser accepts Hole as expression atom).
//   2. Deserialize the hole_role_map_json: [{"id": N, "role": "..."}, ...]
//   3. Walk the parsed AST with hole_visitor; collect role-mismatch errors.
//   4. Build a JSON response of one of the two shapes:
//        { "ok": true,  "ast":   <statements-as-json> }
//        { "ok": false, "errors": [<error-object>, ...] }
//
// The C# side (PolyglotHoleApi.cs) deserializes this into HoleParseResult.

use std::collections::HashMap;
use std::os::raw::c_char;

use serde::Deserialize;

use crate::helpers::{dialect_by_name, err_result, ok_json_result, panic_result, required_arg};
use crate::holes::Role;
use crate::hole_visitor::{visit_statements, HoleError};
use crate::types::{PolyglotResult, STATUS_PARSE_ERROR};

#[derive(Debug, Deserialize)]
struct HoleSpec {
    id: u32,
    role: Role,
}

/// Parse SQL containing «hole:N» sentinels and validate that each hole's
/// declared role matches the role expected by the grammar at its AST position.
///
/// # Arguments
/// - `sql_with_sentinels`: SQL text with «hole:N» sentinels in place of values.
/// - `hole_role_map_json`: JSON array `[{"id": N, "role": "ColumnExpr"}, ...]`.
/// - `dialect`: SQL dialect name (e.g., `"postgres"`).
///
/// # Returns
/// `PolyglotResult` with status 0 in the normal path. `data` contains a JSON
/// payload representing the validation outcome (success or error list).
/// `status` is non-zero only on FFI-level failure (NULL pointer, etc.).
///
/// Caller must free the result with `polyglot_free_result`.
#[no_mangle]
pub extern "C" fn polyglot_parse_with_holes(
    sql_with_sentinels: *const c_char,
    hole_role_map_json: *const c_char,
    dialect: *const c_char,
) -> PolyglotResult {
    match std::panic::catch_unwind(|| {
        parse_with_holes_impl(sql_with_sentinels, hole_role_map_json, dialect)
    }) {
        Ok(result) => result,
        Err(panic) => panic_result(panic),
    }
}

fn parse_with_holes_impl(
    sql_with_sentinels: *const c_char,
    hole_role_map_json: *const c_char,
    dialect: *const c_char,
) -> PolyglotResult {
    let sql = match unsafe { required_arg(sql_with_sentinels, "sql_with_sentinels") } {
        Ok(v) => v,
        Err(r) => return r,
    };
    let role_map_json = match unsafe { required_arg(hole_role_map_json, "hole_role_map_json") } {
        Ok(v) => v,
        Err(r) => return r,
    };
    let dialect_name = match unsafe { required_arg(dialect, "dialect") } {
        Ok(v) => v,
        Err(r) => return r,
    };

    // Resolve dialect.
    let dialect_impl = match dialect_by_name(&dialect_name) {
        Ok(d) => d,
        Err(r) => return r,
    };

    // Parse the role map: [{"id": N, "role": "..."}, ...]
    let specs: Vec<HoleSpec> = match serde_json::from_str(&role_map_json) {
        Ok(v) => v,
        Err(e) => {
            return err_result(
                STATUS_PARSE_ERROR,
                format!("hole_role_map_json is not a valid role-map array: {}", e),
            );
        }
    };
    let role_map: HashMap<u32, Role> = specs.into_iter().map(|s| (s.id, s.role)).collect();

    // Parse the SQL.
    let statements = match dialect_impl.parse(&sql) {
        Ok(stmts) => stmts,
        Err(e) => {
            // SQL parse failed. Report it under the "ok": false shape so the
            // .NET side gets a uniform response.
            let payload = serde_json::json!({
                "ok": false,
                "errors": [{
                    "kind": "parse_error",
                    "message": format!("{}", e),
                    "line": null,
                    "column": null,
                }],
            });
            return ok_json_result(&payload);
        }
    };

    // Walk the AST, collecting role errors.
    let errors = visit_statements(&statements, &role_map);

    let payload = if errors.is_empty() {
        // Success: include the AST as data for debugging / future use.
        let ast_json = serde_json::to_value(&statements).unwrap_or(serde_json::Value::Null);
        serde_json::json!({
            "ok": true,
            "ast": ast_json,
        })
    } else {
        let errs: Vec<serde_json::Value> = errors
            .iter()
            .map(|e| match e {
                HoleError::RoleMismatch {
                    hole_id,
                    expected_roles,
                    declared_role,
                } => serde_json::json!({
                    "kind": "role_mismatch",
                    "hole_id": hole_id,
                    "expected_roles": expected_roles,
                    "declared_role": declared_role,
                }),
                HoleError::UnknownHole { hole_id, message } => serde_json::json!({
                    "kind": "unknown_hole",
                    "hole_id": hole_id,
                    "message": message,
                }),
            })
            .collect();
        serde_json::json!({
            "ok": false,
            "errors": errs,
        })
    };

    ok_json_result(&payload)
}
