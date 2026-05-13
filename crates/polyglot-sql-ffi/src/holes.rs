// holes.rs — Role enum + subsumption lattice for the typed-hole API (POC-002).
//
// Mirrors the C# Role enum in PolyglotHoleApi.cs. JSON serialization uses
// camelCase to match the .NET-side JsonSerializerOptions.

use serde::{Deserialize, Serialize};

/// Grammar-position roles understood by the typed-hole API.
///
/// `Expr` is the wildcard: a position with role Expr accepts any declared
/// role, and a declared role of Expr is accepted at any position. All other
/// roles require an exact match between position and declared.
///
/// This is the simplest acceptance rule that gives precise role-checking
/// for specific declarations (`«hole»:ColumnExpr` placed in WHERE → reject)
/// while keeping `:Expr` as an explicit "I don't know exactly" escape hatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Role {
    Expr,
    ColumnExpr,
    SelectItem,
    WhereClause,
    OrderByExpr,
    GroupByExpr,
    HavingExpr,
    TableRef,
}

impl Role {
    /// Returns true iff a hole declared as `declared` is acceptable at a
    /// grammar position requiring `self`.
    ///
    /// Rule: position `Expr` accepts anything; declared `Expr` is accepted
    /// at any position; otherwise positions and declarations must match
    /// exactly.
    pub fn accepts(self, declared: Role) -> bool {
        self == declared || self == Role::Expr || declared == Role::Expr
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match_accepts() {
        assert!(Role::WhereClause.accepts(Role::WhereClause));
        assert!(Role::ColumnExpr.accepts(Role::ColumnExpr));
        assert!(Role::TableRef.accepts(Role::TableRef));
    }

    #[test]
    fn expr_is_wildcard_on_both_sides() {
        assert!(Role::Expr.accepts(Role::WhereClause));
        assert!(Role::Expr.accepts(Role::ColumnExpr));
        assert!(Role::WhereClause.accepts(Role::Expr));
        assert!(Role::ColumnExpr.accepts(Role::Expr));
    }

    #[test]
    fn specific_roles_dont_accept_other_specifics() {
        // A ColumnExpr declaration in a WhereClause position is a type error
        // (the user said "column expression" but the slot wants a predicate).
        assert!(!Role::WhereClause.accepts(Role::ColumnExpr));
        // And vice versa.
        assert!(!Role::ColumnExpr.accepts(Role::WhereClause));
        // OrderByExpr and SelectItem are also disjoint.
        assert!(!Role::OrderByExpr.accepts(Role::SelectItem));
        assert!(!Role::SelectItem.accepts(Role::OrderByExpr));
    }

    #[test]
    fn table_ref_is_disjoint_from_expression_roles() {
        // TableRef positions only accept declared TableRef (or the Expr
        // wildcard). They specifically reject ColumnExpr / SelectItem / etc.
        assert!(Role::TableRef.accepts(Role::TableRef));
        assert!(Role::TableRef.accepts(Role::Expr));
        assert!(!Role::TableRef.accepts(Role::ColumnExpr));
        assert!(!Role::TableRef.accepts(Role::WhereClause));
        assert!(!Role::ColumnExpr.accepts(Role::TableRef));
    }
}
