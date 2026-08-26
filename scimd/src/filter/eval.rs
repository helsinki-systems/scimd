//! Matching a parsed [`Filter`] against a resource's JSON.
//!
//! Everything here works off the shape of the value rather than off the resource's declared
//! schema, which is what lets a service filter a resource type it assembled by hand as readily as
//! one scimd knows about. The cost is that `caseExact` cannot be honoured, so every string
//! comparison folds case; see the module docs for why that is the right default.

use std::cmp::Ordering;

use serde_json::{Map, Value};

use super::{AttrPath, CompareOp, CompareValue, Filter};

pub(super) fn matches(filter: &Filter, resource: &Value) -> bool {
    match filter {
        Filter::And(left, right) => matches(left, resource) && matches(right, resource),
        Filter::Or(left, right) => matches(left, resource) || matches(right, resource),
        Filter::Not(inner) => !matches(inner, resource),
        Filter::Present(attr) => resolve(resource, attr).into_iter().any(is_present),
        Filter::Compare { attr, op, value } => compare_any(*op, &resolve(resource, attr), value),
        // Every condition has to hold of one and the same element, which is the whole point of a
        // `valuePath` - `emails[type eq "work" and value co "@example.com"]` is not the same
        // question as `emails.type eq "work" and emails.value co "@example.com"`.
        Filter::ValuePath { attr, filter } => resolve(resource, attr)
            .into_iter()
            .any(|element| matches(filter, element)),
    }
}

/// Every value an `attrPath` names, with a multi-valued attribute contributing each of its
/// elements. RFC 7644 section 3.4.2.2 matches a multi-valued attribute when any of its values
/// matches, so flattening here means no operator has to know which attributes are multi-valued.
fn resolve<'a>(resource: &'a Value, path: &AttrPath) -> Vec<&'a Value> {
    let Some(root) = root_for(resource, path) else {
        return vec![];
    };
    let mut found = flatten(member(root, &path.attr));
    if let Some(sub_attr) = &path.sub_attr {
        found = found
            .into_iter()
            .flat_map(|value| flatten(member(value, sub_attr)))
            .collect();
    }
    found
}

/// Where a fully qualified `attrPath` starts looking, or `None` when it names nothing at all.
///
/// An extension's attributes live under the extension's own URN - `models::User::enterprise_user`
/// is serialised under `urn:ietf:params:scim:schemas:extension:enterprise:2.0:User` - so a URN that
/// names a key is descended into. A core attribute's URN names no key at all, since
/// `urn:ietf:params:scim:schemas:core:2.0:User:userName` is just `userName`, so a URN the resource
/// claims in its `schemas` reads at the root instead.
///
/// A URN that is neither is a question about a schema this resource does not have, and the answer
/// is no values rather than the root's. Falling back to the root there would quietly answer a
/// different question: `urn:example:Device:2.0:location eq "berlin"` would match a *core*
/// `location` on every resource that lacks the extension, and no filter could then mean "the
/// extension's `location`". A resource carrying no `schemas` at all - JSON a service assembled by
/// hand, which the rest of this module is careful to keep working - has nothing to check against,
/// so it keeps the permissive reading.
///
/// This is deliberately not [`crate::models::normalize_attr`], which drops the URN unconditionally.
/// That is right for a PATCH path and wrong here: dropping it discards exactly what is needed to
/// find an extension attribute.
fn root_for<'a>(resource: &'a Value, path: &AttrPath) -> Option<&'a Value> {
    let Some(urn) = &path.urn else {
        return Some(resource);
    };
    if let Some(extension) = member(resource, urn).filter(|value| value.is_object()) {
        return Some(extension);
    }
    claims(resource, urn).then_some(resource)
}

/// Whether `resource` names `urn` among its `schemas`, which a resource listing none at all does
/// vacuously.
fn claims(resource: &Value, urn: &str) -> bool {
    let Some(Value::Array(schemas)) = member(resource, "schemas") else {
        return true;
    };
    schemas.iter().any(|schema| {
        schema
            .as_str()
            .is_some_and(|found| found.eq_ignore_ascii_case(urn))
    })
}

/// RFC 7643 section 2.1 makes attribute names case-insensitive. `ATTRNAME` is ASCII by the ABNF,
/// so the fold is too, and an exact hit - which is what every lowercase attribute name gives -
/// costs one lookup.
fn member<'a>(value: &'a Value, name: &str) -> Option<&'a Value> {
    let object = value.as_object()?;
    object.get(name).or_else(|| member_of(object, name))
}

fn member_of<'a>(object: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    object
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))
        .map(|(_, value)| value)
}

fn flatten(value: Option<&Value>) -> Vec<&Value> {
    match value {
        None | Some(Value::Null) => vec![],
        Some(Value::Array(values)) => values.iter().collect(),
        Some(other) => vec![other],
    }
}

/// RFC 7644 section 3.4.2.2: `pr` "matches all values that are not empty, null or unassigned". An
/// unassigned attribute resolves to nothing at all, so it needs no case of its own here.
fn is_present(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(found) => !found.is_empty(),
        Value::Array(values) => !values.is_empty(),
        Value::Object(members) => !members.is_empty(),
        // A number or a boolean is assigned even when it is `0` or `false`.
        Value::Bool(_) | Value::Number(_) => true,
    }
}

/// `ne` asks that *no* value match, rather than that some value differ, so a multi-valued
/// attribute is only `ne` a value when none of its values is that value - and an attribute the
/// resource does not carry satisfies it, which is what makes RFC 7644's own
/// `userType ne "Employee" and not (...)` example read the way it looks.
fn compare_any(op: CompareOp, candidates: &[&Value], wanted: &CompareValue) -> bool {
    match op {
        CompareOp::Ne => !candidates
            .iter()
            .any(|candidate| compare(CompareOp::Eq, candidate, wanted)),
        _ => candidates
            .iter()
            .any(|candidate| compare(op, candidate, wanted)),
    }
}

fn compare(op: CompareOp, candidate: &Value, wanted: &CompareValue) -> bool {
    // A scalar comparison that lands on a complex value falls through to its `value`
    // sub-attribute. RFC 7643 section 2.4 makes `value` the significant content of a multi-valued
    // attribute's element, which is how RFC 7644 section 3.4.2.2 can print `emails co
    // "example.com"` as an example filter - `emails` is an array of objects, and without this the
    // example would never match anything.
    if let Value::Object(members) = candidate {
        return member_of(members, "value").is_some_and(|inner| compare(op, inner, wanted));
    }

    match op {
        CompareOp::Eq => ordering(candidate, wanted) == Some(Ordering::Equal),
        CompareOp::Ne => ordering(candidate, wanted) != Some(Ordering::Equal),
        CompareOp::Co => substring(candidate, wanted, Where::Anywhere),
        CompareOp::Sw => substring(candidate, wanted, Where::Start),
        CompareOp::Ew => substring(candidate, wanted, Where::End),
        CompareOp::Gt => ordered(candidate, wanted).is_some_and(Ordering::is_gt),
        CompareOp::Ge => ordered(candidate, wanted).is_some_and(Ordering::is_ge),
        CompareOp::Lt => ordered(candidate, wanted).is_some_and(Ordering::is_lt),
        CompareOp::Le => ordered(candidate, wanted).is_some_and(Ordering::is_le),
    }
}

/// Which of `co`, `sw` and `ew` is being asked.
#[derive(Clone, Copy)]
enum Where {
    Anywhere,
    Start,
    End,
}

/// `co`, `sw` and `ew` are defined on strings and on nothing else.
///
/// A filter is matched against every value of every resource in a collection, so the ASCII path
/// here is the one that runs: it folds case a byte at a time instead of building two `String`s per
/// value. `ATTRNAME` is ASCII by the ABNF and so is every example RFC 7644 prints, but a value need
/// not be, and one that is not still gets the full Unicode fold.
fn substring(candidate: &Value, wanted: &CompareValue, r#where: Where) -> bool {
    let (Value::String(haystack), CompareValue::String(needle)) = (candidate, wanted) else {
        return false;
    };
    if haystack.is_ascii() && needle.is_ascii() {
        return found_ascii(haystack.as_bytes(), needle.as_bytes(), r#where);
    }
    let (haystack, needle) = (haystack.to_lowercase(), needle.to_lowercase());
    match r#where {
        Where::Anywhere => haystack.contains(&needle),
        Where::Start => haystack.starts_with(&needle),
        Where::End => haystack.ends_with(&needle),
    }
}

fn found_ascii(haystack: &[u8], needle: &[u8], r#where: Where) -> bool {
    let Some(slack) = haystack.len().checked_sub(needle.len()) else {
        return false;
    };
    match r#where {
        // `windows` panics on a width of zero, and every string starts with the empty one anyway.
        Where::Anywhere if needle.is_empty() => true,
        Where::Anywhere => haystack
            .windows(needle.len())
            .any(|window| window.eq_ignore_ascii_case(needle)),
        Where::Start => haystack[..needle.len()].eq_ignore_ascii_case(needle),
        Where::End => haystack[slack..].eq_ignore_ascii_case(needle),
    }
}

/// The ordering `gt`, `ge`, `lt` and `le` ask for. RFC 7644 section 3.4.2.2 leaves them undefined
/// for booleans and for null, so only strings and numbers answer.
fn ordered(candidate: &Value, wanted: &CompareValue) -> Option<Ordering> {
    match (candidate, wanted) {
        (Value::String(_), CompareValue::String(_))
        | (Value::Number(_), CompareValue::Number(_)) => ordering(candidate, wanted),
        _ => None,
    }
}

/// `None` when the two are not comparable at all, which every caller reads as "does not match".
/// A question that cannot be asked of a resource is answered no, never with an error.
///
/// Strings fold case, because RFC 7643 section 7 defaults `caseExact` to false. A `dateTime` is
/// carried as an RFC 3339 string, which orders chronologically as text as long as both sides are
/// written the same way - which is what RFC 7644's `meta.lastModified gt "2011-05-13T04:42:34Z"`
/// example relies on.
fn ordering(candidate: &Value, wanted: &CompareValue) -> Option<Ordering> {
    match (candidate, wanted) {
        (Value::String(found), CompareValue::String(wanted)) => Some(text(found, wanted)),
        (Value::Number(found), CompareValue::Number(wanted)) => number(found, wanted),
        (Value::Bool(found), CompareValue::Bool(wanted)) => Some(found.cmp(wanted)),
        (Value::Null, CompareValue::Null) => Some(Ordering::Equal),
        _ => None,
    }
}

/// Case-folded, and without allocating when both sides are ASCII - see [`substring`] for why that
/// is worth a branch.
fn text(found: &str, wanted: &str) -> Ordering {
    if found.is_ascii() && wanted.is_ascii() {
        return found
            .bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .cmp(wanted.bytes().map(|byte| byte.to_ascii_lowercase()));
    }
    found.to_lowercase().cmp(&wanted.to_lowercase())
}

/// Integers compare as integers.
///
/// `f64` is the obvious way to compare two [`serde_json::Number`]s and it is wrong past 2^53, where
/// distinct integers land on one float: `externalId eq 9007199254740993` would match a resource
/// carrying `9007199254740992`, and `gt` would call the two equal. SCIM has no numeric type of its
/// own beyond JSON's, so a service is free to key on an integer that large.
fn number(found: &serde_json::Number, wanted: &serde_json::Number) -> Option<Ordering> {
    if let (Some(found), Some(wanted)) = (found.as_i64(), wanted.as_i64()) {
        return Some(found.cmp(&wanted));
    }
    if let (Some(found), Some(wanted)) = (found.as_u64(), wanted.as_u64()) {
        return Some(found.cmp(&wanted));
    }
    if found.is_f64() || wanted.is_f64() {
        return found.as_f64()?.partial_cmp(&wanted.as_f64()?);
    }
    // Both are integers that no one type holds: one is past `i64::MAX`, the other is negative.
    Some(if found.as_u64().is_some() {
        Ordering::Greater
    } else {
        Ordering::Less
    })
}
