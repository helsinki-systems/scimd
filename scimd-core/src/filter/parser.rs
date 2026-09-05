//! The RFC 7644 section 3.4.2.2 grammar, in nom.
//!
//! ```abnf
//! FILTER    = attrExp / logExp / valuePath / *1"not" "(" FILTER ")"
//! valuePath = attrPath "[" valFilter "]"
//! valFilter = attrExp / logExp / *1"not" "(" valFilter ")"
//! attrExp   = (attrPath SP "pr") / (attrPath SP compareOp SP compValue)
//! logExp    = FILTER SP ("and" / "or") SP FILTER
//! compValue = false / null / true / number / string   ; rules from JSON (RFC 7159)
//! compareOp = "eq" / "ne" / "co" / "sw" / "ew" / "gt" / "lt" / "ge" / "le"
//! attrPath  = [URI ":"] ATTRNAME *1subAttr
//! ATTRNAME  = ALPHA *(nameChar)
//! nameChar  = "-" / "_" / DIGIT / ALPHA
//! subAttr   = "." ATTRNAME
//! ```
//!
//! Two things the ABNF says that are easy to read past. `not` has no form without parentheses, so
//! `not userName eq "x"` is not a filter. And `valFilter` has no `valuePath` alternative while
//! `FILTER` carries no trailing `subAttr` after one, so neither `emails[a[b pr]]` nor
//! `emails[type eq "work"].value` is a filter - the second is a PATCH path (RFC 7644 section
//! 3.5.2), which is a different production.

use nom::branch::alt;
use nom::bytes::complete::{escaped, is_not, take_while, take_while1};
use nom::character::complete::{anychar, char, multispace0, one_of, satisfy};
use nom::combinator::{all_consuming, map, map_opt, opt, recognize, value};
use nom::error::{Error as NomError, ErrorKind};
use nom::multi::many0;
use nom::sequence::{delimited, preceded};
use nom::{IResult, Parser};

use super::{AttrPath, CompareOp, CompareValue, Filter};

type Parsed<'a, T> = IResult<&'a str, T>;

/// The longest filter that is read at all.
///
/// A filter arrives from the network, and its length is what bounds the depth of the [`Filter`] it
/// parses to: `a pr and a pr and ...` folds to the left, so a chain of N terms is a tree N deep,
/// which is how deep both [`Filter::matches`](super::Filter::matches) and the tree's own
/// `Drop` then recurse. A stack overflow in Rust aborts the process instead of unwinding, so a
/// filter with no size is a way to stop a whole server - `POST /Users/.search` alone will carry two
/// megabytes of one. Real filters run to a few hundred bytes, which leaves room to spare here.
pub const MAX_LENGTH: usize = 8 * 1024;

/// The deepest a filter may nest `(` and `[`.
///
/// [`MAX_LENGTH`] does not bound this closely enough on its own, because a level of nesting costs
/// one byte and several stack frames. RFC 7644's own examples nest one level deep.
pub const MAX_DEPTH: usize = 20;

/// Parse a SCIM filter (RFC 7644 section 3.4.2.2).
///
/// The whole string has to be a filter: trailing text is refused rather than ignored, because a
/// filter half-understood answers a narrower question than the one that was asked.
///
/// A filter longer than [`MAX_LENGTH`], or nesting deeper than [`MAX_DEPTH`], is refused unread
/// rather than parsed; see those constants for why a filter needs a size at all.
///
/// # Errors
///
/// [`crate::Error::InvalidFilter`] if `input` is not a filter, naming the position the grammar
/// broke at. That becomes a `400` carrying `scimType: invalidFilter` and the same detail.
pub fn parse(input: &str) -> crate::Result<Filter> {
    if input.trim().is_empty() {
        return Err(crate::Error::InvalidFilter(
            "the filter is empty (RFC 7644 section 3.4.2.2)".to_owned(),
        ));
    }
    if input.len() > MAX_LENGTH {
        return Err(crate::Error::InvalidFilter(format!(
            "the filter is {} bytes long, and {MAX_LENGTH} is the most that will be read \
             (RFC 7644 section 3.4.2.2)",
            input.len()
        )));
    }

    match all_consuming(delimited(multispace0, top_level, multispace0)).parse(input) {
        Ok((_, filter)) => Ok(filter),
        Err(nom::Err::Error(e) | nom::Err::Failure(e)) => Err(crate::Error::InvalidFilter(
            if e.code == ErrorKind::TooLarge {
                format!(
                    "the filter nests more than {MAX_DEPTH} levels deep at character {} \
                     (RFC 7644 section 3.4.2.2)",
                    character_offset(input, e.input)
                )
            } else {
                describe(input, e.input)
            },
        )),
        Err(nom::Err::Incomplete(_)) => Err(crate::Error::InvalidFilter(format!(
            "the filter `{input}` ends in the middle of an expression"
        ))),
    }
}

fn describe(input: &str, remaining: &str) -> String {
    let position = character_offset(input, remaining);
    let fragment: String = remaining.chars().take(24).collect();
    if fragment.is_empty() {
        format!(
            "the filter ends after {position} characters, in the middle of an expression \
             (RFC 7644 section 3.4.2.2)"
        )
    } else {
        format!(
            "the filter is not valid at character {position}, starting at `{fragment}` \
             (RFC 7644 section 3.4.2.2)"
        )
    }
}

/// Where `remaining` begins, counted in characters rather than in bytes.
///
/// The two part company as soon as a filter carries a non-ASCII string, and a client counting its
/// way through `displayName eq "Munchen" bogus` is counting characters - naming a byte offset to it
/// is naming a position that is not there.
fn character_offset(input: &str, remaining: &str) -> usize {
    let bytes = input.len().saturating_sub(remaining.len());
    input
        .get(..bytes)
        .map_or(bytes, |prefix| prefix.chars().count())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Nesting {
    Filter,
    ValueFilter,
}

/// What the recursive parsers carry down: which production they are inside, and how many `(` and
/// `[` they have descended through on the way.
#[derive(Clone, Copy)]
struct Ctx {
    nesting: Nesting,
    depth: usize,
}

impl Ctx {
    /// One level further in, or `None` once [`MAX_DEPTH`] levels have been spent.
    fn deeper(self, nesting: Nesting) -> Option<Self> {
        (self.depth < MAX_DEPTH).then_some(Self {
            nesting,
            depth: self.depth + 1,
        })
    }
}

/// A [`nom::Err::Failure`] that [`parse`] tells apart from a grammar error by its [`ErrorKind`].
fn too_deep(input: &str) -> nom::Err<NomError<&str>> {
    nom::Err::Failure(NomError::new(input, ErrorKind::TooLarge))
}

fn top_level(input: &str) -> Parsed<'_, Filter> {
    or_expr(
        input,
        Ctx {
            nesting: Nesting::Filter,
            depth: 0,
        },
    )
}

fn or_expr(input: &str, ctx: Ctx) -> Parsed<'_, Filter> {
    let (input, first) = and_expr(input, ctx)?;
    let (input, rest) = many0(preceded(
        (multispace0, keyword("or"), multispace0),
        move |i| and_expr(i, ctx),
    ))
    .parse(input)?;
    Ok((
        input,
        rest.into_iter().fold(first, |left, right| {
            Filter::Or(Box::new(left), Box::new(right))
        }),
    ))
}

fn and_expr(input: &str, ctx: Ctx) -> Parsed<'_, Filter> {
    let (input, first) = not_expr(input, ctx)?;
    let (input, rest) = many0(preceded(
        (multispace0, keyword("and"), multispace0),
        move |i| not_expr(i, ctx),
    ))
    .parse(input)?;
    Ok((
        input,
        rest.into_iter().fold(first, |left, right| {
            Filter::And(Box::new(left), Box::new(right))
        }),
    ))
}

fn not_expr(input: &str, ctx: Ctx) -> Parsed<'_, Filter> {
    if let Ok((rest, ())) = keyword("not").parse(input) {
        let (rest, _) = multispace0(rest)?;
        if rest.starts_with('(') {
            let (rest, inner) = commit(group(rest, ctx))?;
            return Ok((rest, Filter::Not(Box::new(inner))));
        }
    }
    primary(input, ctx)
}

fn primary(input: &str, ctx: Ctx) -> Parsed<'_, Filter> {
    if input.starts_with('(') {
        return commit(group(input, ctx));
    }
    attr_exp_or_value_path(input, ctx)
}

fn group(input: &str, ctx: Ctx) -> Parsed<'_, Filter> {
    let Some(inner) = ctx.deeper(ctx.nesting) else {
        return Err(too_deep(input));
    };
    delimited(
        (char('('), multispace0),
        move |i| or_expr(i, inner),
        (multispace0, char(')')),
    )
    .parse(input)
}

fn attr_exp_or_value_path(input: &str, ctx: Ctx) -> Parsed<'_, Filter> {
    let (rest, attr) = attr_path(input)?;
    let (rest, _) = multispace0(rest)?;

    if rest.starts_with('[') {
        if ctx.nesting == Nesting::ValueFilter {
            return Err(nom::Err::Failure(NomError::new(rest, ErrorKind::Char)));
        }
        let Some(nested) = ctx.deeper(Nesting::ValueFilter) else {
            return Err(too_deep(rest));
        };
        let (rest, inner) = commit(
            delimited(
                (char('['), multispace0),
                |i| or_expr(i, nested),
                (multispace0, char(']')),
            )
            .parse(rest),
        )?;
        return Ok((
            rest,
            Filter::ValuePath {
                attr,
                filter: Box::new(inner),
            },
        ));
    }

    let operator = rest;
    let (rest, word) = word(rest)?;
    if word.eq_ignore_ascii_case("pr") {
        return Ok((rest, Filter::Present(attr)));
    }
    let Some(op) = CompareOp::from_keyword(word) else {
        return Err(nom::Err::Failure(NomError::new(operator, ErrorKind::Tag)));
    };
    let (rest, _) = multispace0(rest)?;
    let (rest, value) = commit(compare_value(rest))?;
    Ok((rest, Filter::Compare { attr, op, value }))
}

fn commit<T>(result: Parsed<'_, T>) -> Parsed<'_, T> {
    match result {
        Err(nom::Err::Error(e)) => Err(nom::Err::Failure(e)),
        other => other,
    }
}

fn word(input: &str) -> Parsed<'_, &str> {
    take_while1(|c: char| c.is_ascii_alphabetic()).parse(input)
}

fn keyword<'a>(
    expected: &'static str,
) -> impl Parser<&'a str, Output = (), Error = NomError<&'a str>> {
    map_opt(word, move |found: &str| {
        found.eq_ignore_ascii_case(expected).then_some(())
    })
}

fn attr_path(input: &str) -> Parsed<'_, AttrPath> {
    map_opt(
        recognize((
            satisfy(|c: char| c.is_ascii_alphabetic()),
            take_while(|c: char| is_name_char(c) || c == '.' || c == ':'),
        )),
        split_attr_path,
    )
    .parse(input)
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '-' || c == '_'
}

fn split_attr_path(token: &str) -> Option<AttrPath> {
    let (urn, rest) = match token.rfind(':') {
        Some(colon) => (Some(token[..colon].to_owned()), &token[colon + 1..]),
        None => (None, token),
    };
    let mut parts = rest.split('.');
    let attr = attr_name(parts.next()?)?;
    let sub_attr = match parts.next() {
        Some(name) => Some(attr_name(name)?),
        None => None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(AttrPath {
        urn,
        attr,
        sub_attr,
    })
}

fn attr_name(name: &str) -> Option<String> {
    let mut chars = name.chars();
    chars.next().filter(char::is_ascii_alphabetic)?;
    chars.all(is_name_char).then(|| name.to_owned())
}

fn compare_value(input: &str) -> Parsed<'_, CompareValue> {
    alt((
        map(json_string, CompareValue::String),
        map(json_number, CompareValue::Number),
        value(CompareValue::Bool(true), keyword("true")),
        value(CompareValue::Bool(false), keyword("false")),
        value(CompareValue::Null, keyword("null")),
    ))
    .parse(input)
}

fn json_string(input: &str) -> Parsed<'_, String> {
    map_opt(
        recognize((
            char('"'),
            opt(escaped(is_not("\"\\"), '\\', anychar)),
            char('"'),
        )),
        |span: &str| serde_json::from_str::<String>(span).ok(),
    )
    .parse(input)
}

fn json_number(input: &str) -> Parsed<'_, serde_json::Number> {
    map_opt(
        recognize((
            opt(char('-')),
            take_while1(|c: char| c.is_ascii_digit()),
            opt((char('.'), take_while1(|c: char| c.is_ascii_digit()))),
            opt((
                one_of("eE"),
                opt(one_of("+-")),
                take_while1(|c: char| c.is_ascii_digit()),
            )),
        )),
        |span: &str| serde_json::from_str::<serde_json::Number>(span).ok(),
    )
    .parse(input)
}
