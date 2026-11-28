use skotos_domain::{PolicyEffect, PolicyStatement};

/// Outcome of evaluating a principal's Custom RBAC policy statements.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyDecision {
    ExplicitAllow,
    ExplicitDeny,
    NoMatch,
}

/// Evaluates declarative resource-path allow/deny policy statements for an action and resource path.
///
/// Precedence rules (ADR-0002 / LaunchDarkly-compatible):
/// 1. Any matching `Deny` statement immediately wins (`ExplicitDeny`), overriding all `Allow` rules.
/// 2. Otherwise, any matching `Allow` statement yields `ExplicitAllow`.
/// 3. If no statement matches, yields `NoMatch` (caller falls back to built-in role matrix or denies).
pub fn evaluate_policies(
    statements: &[PolicyStatement],
    action: &str,
    resource_path: &str,
) -> PolicyDecision {
    let mut allowed = false;
    for stmt in statements {
        if !matches_action(stmt, action) || !matches_resource(stmt, resource_path) {
            continue;
        }
        match stmt.effect {
            PolicyEffect::Deny => return PolicyDecision::ExplicitDeny,
            PolicyEffect::Allow => allowed = true,
        }
    }
    if allowed {
        PolicyDecision::ExplicitAllow
    } else {
        PolicyDecision::NoMatch
    }
}

fn matches_action(stmt: &PolicyStatement, action: &str) -> bool {
    let has_pos = !stmt.actions.is_empty();
    let has_neg = !stmt.not_actions.is_empty();

    if has_pos && !stmt.actions.iter().any(|pat| glob_match(pat, action)) {
        return false;
    }
    if has_neg && stmt.not_actions.iter().any(|pat| glob_match(pat, action)) {
        return false;
    }
    has_pos || has_neg
}

fn matches_resource(stmt: &PolicyStatement, resource_path: &str) -> bool {
    let has_pos = !stmt.resources.is_empty();
    let has_neg = !stmt.not_resources.is_empty();

    if has_pos
        && !stmt
            .resources
            .iter()
            .any(|pat| glob_match(pat, resource_path))
    {
        return false;
    }
    if has_neg
        && stmt
            .not_resources
            .iter()
            .any(|pat| glob_match(pat, resource_path))
    {
        return false;
    }
    has_pos || has_neg
}

/// Iterative linear-time glob matcher supporting `*` (0+ chars) and `?` (1 char).
/// Safe against catastrophic backtracking on untrusted policy patterns.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0usize, 0usize);
    let mut star_idx: Option<usize> = None;
    let mut match_idx = 0usize;

    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star_idx = Some(pi);
            match_idx = ti;
            pi += 1;
        } else if let Some(si) = star_idx {
            pi = si + 1;
            match_idx += 1;
            ti = match_idx;
        } else {
            return false;
        }
    }

    while pi < p.len() && p[pi] == '*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stmt(effect: PolicyEffect, resources: &[&str], actions: &[&str]) -> PolicyStatement {
        PolicyStatement {
            effect,
            resources: resources.iter().map(|s| (*s).to_string()).collect(),
            not_resources: vec![],
            actions: actions.iter().map(|s| (*s).to_string()).collect(),
            not_actions: vec![],
        }
    }

    #[test]
    fn deny_overrides_allow_on_protected_production_flags() {
        let policies = vec![
            stmt(
                PolicyEffect::Allow,
                &["proj/*:env/*:flag/*"],
                &["patchFlag", "updateOn"],
            ),
            stmt(
                PolicyEffect::Deny,
                &["proj/*:env/production:flag/*"],
                &["patchFlag"],
            ),
        ];

        assert_eq!(
            evaluate_policies(&policies, "patchFlag", "proj/default:env/staging:flag/beta"),
            PolicyDecision::ExplicitAllow
        );
        assert_eq!(
            evaluate_policies(
                &policies,
                "patchFlag",
                "proj/default:env/production:flag/beta"
            ),
            PolicyDecision::ExplicitDeny
        );
    }

    #[test]
    fn supports_not_resources_and_not_actions() {
        let policies = vec![PolicyStatement {
            effect: PolicyEffect::Allow,
            resources: vec!["proj/default:env/*:flag/*".to_string()],
            not_resources: vec!["proj/default:env/production:flag/*".to_string()],
            actions: vec!["*".to_string()],
            not_actions: vec!["deleteFlag".to_string()],
        }];

        assert_eq!(
            evaluate_policies(&policies, "patchFlag", "proj/default:env/dev:flag/checkout"),
            PolicyDecision::ExplicitAllow
        );
        assert_eq!(
            evaluate_policies(
                &policies,
                "deleteFlag",
                "proj/default:env/dev:flag/checkout"
            ),
            PolicyDecision::NoMatch
        );
        assert_eq!(
            evaluate_policies(
                &policies,
                "patchFlag",
                "proj/default:env/production:flag/checkout"
            ),
            PolicyDecision::NoMatch
        );
    }
}
