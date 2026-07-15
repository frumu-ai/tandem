use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateResult {
    Match,
    NoMatch,
    Indeterminate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateValueType {
    String,
    Boolean,
    Integer,
    Decimal,
    CurrencyCode,
    Host,
    EmailDomain,
    Path,
    Repository,
    ArrayLength,
    Exists,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateOperator {
    Equals,
    NotEquals,
    In,
    NotIn,
    StartsWith,
    EndsWith,
    LessThan,
    LessThanOrEqual,
    GreaterThan,
    GreaterThanOrEqual,
    IsSubdomainOf,
    NotSubdomainOf,
    Within,
    NotWithin,
    OwnerEquals,
    NameEquals,
    Exists,
    NotExists,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredicateCondition {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub condition_id: Option<String>,
    pub selector: String,
    pub value_type: PredicateValueType,
    pub operator: PredicateOperator,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub operand: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum PredicateExpression {
    All { all: Vec<PredicateExpression> },
    Any { any: Vec<PredicateExpression> },
    Not { not: Box<PredicateExpression> },
    Condition { condition: PredicateCondition },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PermissionPredicate {
    #[serde(default = "predicate_version")]
    pub expression_version: String,
    #[serde(flatten)]
    pub expression: PredicateExpression,
}

fn predicate_version() -> String {
    "permission_predicates/v1".to_string()
}

impl PermissionPredicate {
    pub fn validate(&self) -> Vec<String> {
        let mut errors = Vec::new();
        if self.expression_version != "permission_predicates/v1" {
            errors.push("expression_version must be permission_predicates/v1".to_string());
        }
        let mut conditions = 0usize;
        validate_expression(&self.expression, 1, &mut conditions, &mut errors);
        if conditions > 32 {
            errors.push("predicate may contain at most 32 conditions".to_string());
        }
        errors
    }

    pub fn evaluate(&self, arguments: &Value) -> PredicateResult {
        if !self.validate().is_empty() {
            return PredicateResult::Indeterminate;
        }
        evaluate_expression(&self.expression, arguments)
    }
}

fn validate_expression(
    expression: &PredicateExpression,
    depth: usize,
    conditions: &mut usize,
    errors: &mut Vec<String>,
) {
    if depth > 4 {
        errors.push("predicate nesting depth may not exceed four".to_string());
        return;
    }
    match expression {
        PredicateExpression::All { all } | PredicateExpression::Any { any: all } => {
            if all.is_empty() || all.len() > 16 {
                errors.push("all/any groups require between 1 and 16 children".to_string());
            }
            for child in all {
                validate_expression(child, depth + 1, conditions, errors);
            }
        }
        PredicateExpression::Not { not } => {
            if !matches!(not.as_ref(), PredicateExpression::Condition { .. }) {
                errors.push("not may wrap only a condition in v1".to_string());
            }
            validate_expression(not, depth + 1, conditions, errors);
        }
        PredicateExpression::Condition { condition } => {
            *conditions += 1;
            validate_condition(condition, errors);
        }
    }
}

fn validate_condition(condition: &PredicateCondition, errors: &mut Vec<String>) {
    if condition.selector.len() > 512
        || (!condition.selector.is_empty() && !condition.selector.starts_with('/'))
        || condition.selector.split('/').skip(1).count() > 12
        || condition
            .selector
            .split('/')
            .skip(1)
            .any(|segment| segment.starts_with("__"))
    {
        errors.push(format!("invalid selector `{}`", condition.selector));
    }
    if matches!(
        condition.operator,
        PredicateOperator::In | PredicateOperator::NotIn
    ) {
        match condition.operand.as_array() {
            Some(values) if !values.is_empty() && values.len() <= 16 => {}
            _ => errors.push("in/not_in operands require an array of 1 to 16 values".to_string()),
        }
    }
    if !operator_supports_value_type(condition.operator, condition.value_type) {
        errors.push(format!(
            "operator `{:?}` is not valid for value type `{:?}`",
            condition.operator, condition.value_type
        ));
        return;
    }
    if !operand_is_valid(condition) {
        errors.push(format!(
            "operand is not valid for operator `{:?}` and value type `{:?}`",
            condition.operator, condition.value_type
        ));
    }
}

fn operator_supports_value_type(
    operator: PredicateOperator,
    value_type: PredicateValueType,
) -> bool {
    use PredicateOperator as O;
    use PredicateValueType as V;
    match operator {
        O::Exists | O::NotExists => true,
        O::Equals | O::NotEquals | O::In | O::NotIn => value_type != V::Exists,
        O::StartsWith | O::EndsWith => matches!(value_type, V::String | V::CurrencyCode),
        O::LessThan | O::LessThanOrEqual | O::GreaterThan | O::GreaterThanOrEqual => {
            matches!(value_type, V::Integer | V::Decimal | V::ArrayLength)
        }
        O::IsSubdomainOf | O::NotSubdomainOf => matches!(value_type, V::Host | V::EmailDomain),
        O::Within | O::NotWithin => value_type == V::Path,
        O::OwnerEquals | O::NameEquals => value_type == V::Repository,
    }
}

fn operand_is_valid(condition: &PredicateCondition) -> bool {
    use PredicateOperator as O;
    match condition.operator {
        O::Exists | O::NotExists => true,
        O::In | O::NotIn => condition.operand.as_array().is_some_and(|values| {
            values
                .iter()
                .all(|value| normalize_operand(value, condition.value_type).is_some())
        }),
        O::OwnerEquals | O::NameEquals => condition.operand.as_str().is_some_and(|value| {
            !value.is_empty() && !value.contains('/') && !value.contains('\\')
        }),
        _ => normalize_operand(&condition.operand, condition.value_type).is_some(),
    }
}

fn evaluate_expression(expression: &PredicateExpression, arguments: &Value) -> PredicateResult {
    match expression {
        PredicateExpression::All { all } => {
            let results = all.iter().map(|item| evaluate_expression(item, arguments));
            fold_all(results)
        }
        PredicateExpression::Any { any } => {
            let results = any.iter().map(|item| evaluate_expression(item, arguments));
            fold_any(results)
        }
        PredicateExpression::Not { not } => match evaluate_expression(not, arguments) {
            PredicateResult::Match => PredicateResult::NoMatch,
            PredicateResult::NoMatch => PredicateResult::Match,
            PredicateResult::Indeterminate => PredicateResult::Indeterminate,
        },
        PredicateExpression::Condition { condition } => evaluate_condition(condition, arguments),
    }
}

fn fold_all(results: impl Iterator<Item = PredicateResult>) -> PredicateResult {
    let mut indeterminate = false;
    for result in results {
        match result {
            PredicateResult::NoMatch => return PredicateResult::NoMatch,
            PredicateResult::Indeterminate => indeterminate = true,
            PredicateResult::Match => {}
        }
    }
    if indeterminate {
        PredicateResult::Indeterminate
    } else {
        PredicateResult::Match
    }
}

fn fold_any(results: impl Iterator<Item = PredicateResult>) -> PredicateResult {
    let mut indeterminate = false;
    for result in results {
        match result {
            PredicateResult::Match => return PredicateResult::Match,
            PredicateResult::Indeterminate => indeterminate = true,
            PredicateResult::NoMatch => {}
        }
    }
    if indeterminate {
        PredicateResult::Indeterminate
    } else {
        PredicateResult::NoMatch
    }
}

fn evaluate_condition(condition: &PredicateCondition, arguments: &Value) -> PredicateResult {
    let selected = if condition.selector.is_empty() {
        Some(arguments)
    } else {
        arguments.pointer(&condition.selector)
    };
    if condition.operator == PredicateOperator::Exists {
        return if selected.is_some() {
            PredicateResult::Match
        } else {
            PredicateResult::NoMatch
        };
    }
    if condition.operator == PredicateOperator::NotExists {
        return if selected.is_none() {
            PredicateResult::Match
        } else {
            PredicateResult::NoMatch
        };
    }
    let Some(selected) = selected else {
        return PredicateResult::Indeterminate;
    };
    let Some(actual) = normalize_value(selected, condition.value_type) else {
        return PredicateResult::Indeterminate;
    };
    let result = match condition.operator {
        PredicateOperator::Equals => {
            compare_equal(&actual, &condition.operand, condition.value_type)
        }
        PredicateOperator::NotEquals => {
            compare_equal(&actual, &condition.operand, condition.value_type).map(|v| !v)
        }
        PredicateOperator::In | PredicateOperator::NotIn => {
            condition.operand.as_array().map(|values| {
                let found = values
                    .iter()
                    .any(|value| compare_equal(&actual, value, condition.value_type) == Some(true));
                if condition.operator == PredicateOperator::NotIn {
                    !found
                } else {
                    found
                }
            })
        }
        PredicateOperator::StartsWith => {
            string_operand(&actual, &condition.operand, condition.value_type)
                .map(|(a, b)| a.starts_with(&b))
        }
        PredicateOperator::EndsWith => {
            string_operand(&actual, &condition.operand, condition.value_type)
                .map(|(a, b)| a.ends_with(&b))
        }
        PredicateOperator::LessThan => {
            numeric_operand(&actual, &condition.operand).map(|(a, b)| a < b)
        }
        PredicateOperator::LessThanOrEqual => {
            numeric_operand(&actual, &condition.operand).map(|(a, b)| a <= b)
        }
        PredicateOperator::GreaterThan => {
            numeric_operand(&actual, &condition.operand).map(|(a, b)| a > b)
        }
        PredicateOperator::GreaterThanOrEqual => {
            numeric_operand(&actual, &condition.operand).map(|(a, b)| a >= b)
        }
        PredicateOperator::IsSubdomainOf | PredicateOperator::NotSubdomainOf => {
            string_operand(&actual, &condition.operand, condition.value_type).map(|(a, b)| {
                let matches = a == b || a.ends_with(&format!(".{b}"));
                if condition.operator == PredicateOperator::NotSubdomainOf {
                    !matches
                } else {
                    matches
                }
            })
        }
        PredicateOperator::Within | PredicateOperator::NotWithin => {
            string_operand(&actual, &condition.operand, condition.value_type).map(|(a, b)| {
                let matches = a == b || a.starts_with(&format!("{}/", b.trim_end_matches('/')));
                if condition.operator == PredicateOperator::NotWithin {
                    !matches
                } else {
                    matches
                }
            })
        }
        PredicateOperator::OwnerEquals => repository_part(&actual, 0, &condition.operand),
        PredicateOperator::NameEquals => repository_part(&actual, 1, &condition.operand),
        PredicateOperator::Exists | PredicateOperator::NotExists => unreachable!(),
    };
    match result {
        Some(true) => PredicateResult::Match,
        Some(false) => PredicateResult::NoMatch,
        None => PredicateResult::Indeterminate,
    }
}

fn normalize_value(value: &Value, value_type: PredicateValueType) -> Option<Value> {
    match value_type {
        PredicateValueType::String => value.as_str().map(|v| Value::String(v.to_string())),
        PredicateValueType::Boolean => value.as_bool().map(Value::Bool),
        PredicateValueType::Integer => value.as_i64().map(Into::into),
        PredicateValueType::Decimal => canonical_decimal(value).map(Value::String),
        PredicateValueType::CurrencyCode => value.as_str().and_then(|value| {
            (value.len() == 3 && value.chars().all(|ch| ch.is_ascii_alphabetic()))
                .then(|| Value::String(value.to_ascii_uppercase()))
        }),
        PredicateValueType::Host | PredicateValueType::EmailDomain => value
            .as_str()
            .and_then(normalize_host_like)
            .map(Value::String),
        PredicateValueType::Path => value.as_str().and_then(normalize_path).map(Value::String),
        PredicateValueType::Repository => value
            .as_str()
            .and_then(normalize_repository)
            .map(Value::String),
        PredicateValueType::ArrayLength => value.as_array().map(|v| Value::from(v.len() as u64)),
        PredicateValueType::Exists => Some(value.clone()),
    }
}

fn compare_equal(actual: &Value, operand: &Value, value_type: PredicateValueType) -> Option<bool> {
    let operand = normalize_operand(operand, value_type)?;
    Some(actual == &operand)
}

fn normalize_operand(value: &Value, value_type: PredicateValueType) -> Option<Value> {
    if value_type == PredicateValueType::ArrayLength {
        return value.as_u64().map(Value::from);
    }
    normalize_value(value, value_type)
}

fn string_operand(
    actual: &Value,
    operand: &Value,
    value_type: PredicateValueType,
) -> Option<(String, String)> {
    let operand = normalize_operand(operand, value_type)?;
    Some((actual.as_str()?.to_string(), operand.as_str()?.to_string()))
}

fn numeric_operand(actual: &Value, operand: &Value) -> Option<(i128, i128)> {
    Some((decimal(actual)?, decimal(operand)?))
}

const DECIMAL_SCALE_DIGITS: usize = 12;

fn decimal(value: &Value) -> Option<i128> {
    let owned;
    let raw = if let Some(value) = value.as_str() {
        value
    } else if value.is_number() {
        owned = value.to_string();
        &owned
    } else {
        return None;
    };
    let raw = raw.trim();
    let (negative, unsigned) = raw
        .strip_prefix('-')
        .map(|value| (true, value))
        .or_else(|| raw.strip_prefix('+').map(|value| (false, value)))
        .unwrap_or((false, raw));
    let (integer, fraction) = unsigned.split_once('.').unwrap_or((unsigned, ""));
    if integer.is_empty()
        || fraction.len() > DECIMAL_SCALE_DIGITS
        || !integer.chars().all(|ch| ch.is_ascii_digit())
        || !fraction.chars().all(|ch| ch.is_ascii_digit())
    {
        return None;
    }
    let significant_digits = integer.trim_start_matches('0').len() + fraction.len();
    if significant_digits > 38 {
        return None;
    }
    let integer = integer.parse::<i128>().ok()?;
    let mut fraction_digits = fraction.to_string();
    fraction_digits.extend(std::iter::repeat_n(
        '0',
        DECIMAL_SCALE_DIGITS - fraction.len(),
    ));
    let fraction = fraction_digits.parse::<i128>().ok()?;
    let scale = 10_i128.pow(DECIMAL_SCALE_DIGITS as u32);
    let scaled = integer.checked_mul(scale)?.checked_add(fraction)?;
    Some(if negative {
        scaled.checked_neg()?
    } else {
        scaled
    })
}

fn canonical_decimal(value: &Value) -> Option<String> {
    let scaled = decimal(value)?;
    let negative = scaled < 0;
    let absolute = scaled.checked_abs()?;
    let scale = 10_i128.pow(DECIMAL_SCALE_DIGITS as u32);
    let integer = absolute / scale;
    let fraction = absolute % scale;
    let mut rendered = if fraction == 0 {
        integer.to_string()
    } else {
        let fraction = format!("{fraction:0width$}", width = DECIMAL_SCALE_DIGITS)
            .trim_end_matches('0')
            .to_string();
        format!("{integer}.{fraction}")
    };
    if negative {
        rendered.insert(0, '-');
    }
    Some(rendered)
}

fn normalize_host_like(value: &str) -> Option<String> {
    let domain = value
        .rsplit_once('@')
        .map(|(_, domain)| domain)
        .unwrap_or(value);
    let domain = domain
        .split("://")
        .nth(1)
        .unwrap_or(domain)
        .split('/')
        .next()?
        .split(':')
        .next()?
        .trim_end_matches('.')
        .to_ascii_lowercase();
    (!domain.is_empty() && domain.split('.').all(|part| !part.is_empty())).then_some(domain)
}

fn normalize_path(value: &str) -> Option<String> {
    if value.contains('\0') {
        return None;
    }
    let mut parts = Vec::new();
    let normalized = value.replace('\\', "/");
    for part in normalized.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            value => parts.push(value),
        }
    }
    Some(parts.join("/"))
}

fn normalize_repository(value: &str) -> Option<String> {
    let value = value.trim_end_matches('/').trim_end_matches(".git");
    let path = value
        .split("://")
        .nth(1)
        .map(|v| v.split_once('/').map(|(_, path)| path).unwrap_or(v))
        .or_else(|| value.split_once(':').map(|(_, path)| path))
        .unwrap_or(value);
    let mut segments = path.split('/').filter(|part| !part.is_empty());
    let owner = segments.next()?;
    let name = segments.next()?;
    (segments.next().is_none()).then(|| {
        format!(
            "{}/{}",
            owner.to_ascii_lowercase(),
            name.to_ascii_lowercase()
        )
    })
}

fn repository_part(actual: &Value, index: usize, operand: &Value) -> Option<bool> {
    let actual = actual.as_str()?.split('/').nth(index)?.to_ascii_lowercase();
    Some(actual == operand.as_str()?.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn condition(
        selector: &str,
        operator: PredicateOperator,
        operand: Value,
    ) -> PermissionPredicate {
        PermissionPredicate {
            expression_version: predicate_version(),
            expression: PredicateExpression::Condition {
                condition: PredicateCondition {
                    condition_id: None,
                    selector: selector.to_string(),
                    value_type: PredicateValueType::String,
                    operator,
                    operand,
                },
            },
        }
    }

    #[test]
    fn missing_fields_and_not_preserve_fail_closed_indeterminate() {
        let inner = condition("/recipient", PredicateOperator::Equals, json!("allowed"));
        let predicate = PermissionPredicate {
            expression_version: predicate_version(),
            expression: PredicateExpression::Not {
                not: Box::new(inner.expression),
            },
        };
        assert_eq!(
            predicate.evaluate(&json!({})),
            PredicateResult::Indeterminate
        );
    }

    #[test]
    fn host_and_amount_predicates_are_argument_aware() {
        let predicate: PermissionPredicate = serde_json::from_value(json!({
            "expression_version": "permission_predicates/v1",
            "all": [
                {"condition": {"selector":"/recipient","value_type":"email_domain","operator":"is_subdomain_of","operand":"example.com"}},
                {"condition": {"selector":"/amount","value_type":"decimal","operator":"less_than","operand":"10000"}}
            ]
        })).unwrap();
        assert_eq!(
            predicate.evaluate(&json!({"recipient":"a@team.example.com","amount":"9999.99"})),
            PredicateResult::Match
        );
        assert_eq!(
            predicate.evaluate(&json!({"recipient":"a@outside.test","amount":"9999.99"})),
            PredicateResult::NoMatch
        );
    }

    #[test]
    fn decimal_comparisons_are_fixed_precision() {
        let predicate: PermissionPredicate = serde_json::from_value(json!({
            "expression_version": "permission_predicates/v1",
            "condition": {
                "selector":"/amount",
                "value_type":"decimal",
                "operator":"equals",
                "operand":"9007199254740993.000000000001"
            }
        }))
        .unwrap();
        assert_eq!(
            predicate.evaluate(&json!({"amount":"9007199254740993.000000000001"})),
            PredicateResult::Match
        );
        assert_eq!(
            predicate.evaluate(&json!({"amount":"9007199254740993.000000000002"})),
            PredicateResult::NoMatch
        );
    }

    #[test]
    fn incompatible_operator_types_fail_validation_and_evaluation_closed() {
        let predicate: PermissionPredicate = serde_json::from_value(json!({
            "expression_version": "permission_predicates/v1",
            "condition": {
                "selector":"/amount",
                "value_type":"decimal",
                "operator":"is_subdomain_of",
                "operand":"example.com"
            }
        }))
        .unwrap();
        assert!(!predicate.validate().is_empty());
        assert_eq!(
            predicate.evaluate(&json!({"amount":"10"})),
            PredicateResult::Indeterminate
        );
    }

    #[test]
    fn canonicalized_operands_match_canonicalized_arguments() {
        let host: PermissionPredicate = serde_json::from_value(json!({
            "expression_version": "permission_predicates/v1",
            "condition": {
                "selector":"/recipient",
                "value_type":"email_domain",
                "operator":"is_subdomain_of",
                "operand":"EXAMPLE.COM."
            }
        }))
        .unwrap();
        assert_eq!(
            host.evaluate(&json!({"recipient":"user@Team.Example.Com"})),
            PredicateResult::Match
        );

        let path: PermissionPredicate = serde_json::from_value(json!({
            "expression_version": "permission_predicates/v1",
            "condition": {
                "selector":"/path",
                "value_type":"path",
                "operator":"within",
                "operand":"/repo/./src"
            }
        }))
        .unwrap();
        assert_eq!(
            path.evaluate(&json!({"path":"/repo/src/lib.rs"})),
            PredicateResult::Match
        );
    }
}
