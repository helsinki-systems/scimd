use serde_json::{Value, json};

use super::{
    AttrPath, CompareOp, CompareValue, Filter, MAX_DEPTH, MAX_LENGTH, parse, patch_values,
};
use crate::models::patchop::Operation;
use crate::{Error, Resource};

const RFC_EXAMPLES: &[&str] = &[
    r#"userName eq "bjensen""#,
    r#"name.familyName co "O'Malley""#,
    r#"userName sw "J""#,
    r#"urn:ietf:params:scim:schemas:core:2.0:User:userName sw "J""#,
    "title pr",
    r#"meta.lastModified gt "2011-05-13T04:42:34Z""#,
    r#"meta.lastModified ge "2011-05-13T04:42:34Z""#,
    r#"meta.lastModified lt "2011-05-13T04:42:34Z""#,
    r#"meta.lastModified le "2011-05-13T04:42:34Z""#,
    r#"title pr and userType eq "Employee""#,
    r#"title pr or userType eq "Intern""#,
    r#"schemas eq "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User""#,
    r#"userType eq "Employee" and (emails co "example.com" or emails.value co "example.org")"#,
    r#"userType ne "Employee" and not (emails co "example.com" or emails.value co "example.org")"#,
    r#"userType eq "Employee" and (emails.type eq "work")"#,
    r#"userType eq "Employee" and emails[type eq "work" and value co "@example.com"]"#,
    r#"emails[type eq "work" and value co "@example.com"] or ims[type eq "xmpp" and value co "@foo.com"]"#,
];

fn parsed(filter: &str) -> Filter {
    parse(filter).unwrap_or_else(|e| panic!("`{filter}` is a filter, but: {e}"))
}

fn rejected(filter: &str) -> String {
    match parse(filter) {
        Ok(parsed) => panic!("`{filter}` is not a filter, but parsed as {parsed:?}"),
        Err(Error::InvalidFilter(detail)) => detail,
        Err(other) => panic!("`{filter}` should be an invalid filter, but gave {other}"),
    }
}

fn path(attr: &str) -> AttrPath {
    AttrPath {
        urn: None,
        attr: attr.to_owned(),
        sub_attr: None,
    }
}

fn present(attr: &str) -> Filter {
    Filter::Present(path(attr))
}

fn bjensen() -> Value {
    let user = crate::models::User {
        schemas: vec![
            crate::models::User::SCHEMA.to_owned(),
            crate::models::enterprise_user::SCHEMA.to_owned(),
        ],
        id: Some("2819c223-7f76-453a-919d-413861904646".to_owned()),
        user_name: "bjensen".to_owned(),
        name: Some(crate::models::user::Name {
            family_name: Some("O'Malley".to_owned()),
            given_name: Some("Barbara".to_owned()),
            ..Default::default()
        }),
        title: Some("Tour Guide".to_owned()),
        user_type: Some("Employee".to_owned()),
        active: Some(false),
        nick_name: Some(String::new()),
        emails: Some(vec![
            crate::models::user::Email {
                value: Some("bjensen@example.com".to_owned()),
                r#type: Some("work".to_owned()),
                primary: Some(true),
                ..Default::default()
            },
            crate::models::user::Email {
                value: Some("barbara@example.org".to_owned()),
                r#type: Some("home".to_owned()),
                ..Default::default()
            },
        ]),
        meta: Some(crate::models::Meta {
            last_modified: Some("2012-05-13T04:42:34Z".to_owned()),
            ..Default::default()
        }),
        enterprise_user: Some(crate::models::enterprise_user::EnterpriseUser {
            department: Some("Tour Operations".to_owned()),
            employee_number: Some("701984".to_owned()),
            ..Default::default()
        }),
        ..Default::default()
    };
    serde_json::to_value(&user).expect("a user serializes")
}

fn matches(filter: &str, resource: &Value) -> bool {
    parsed(filter).matches(resource)
}

#[test]
fn every_filter_rfc_7644_prints_as_an_example_parses() {
    for filter in RFC_EXAMPLES {
        parse(filter).unwrap_or_else(|e| panic!("RFC 7644 prints `{filter}`, but: {e}"));
    }
}

#[test]
fn an_operator_keyword_may_be_written_in_any_case() {
    assert_eq!(
        parsed(r#"userName EQ "bjensen""#),
        parsed(r#"userName eq "bjensen""#),
    );
    assert_eq!(
        parsed("title PR AND userName PR"),
        parsed("title pr and userName pr"),
    );
}

#[test]
fn an_attribute_name_keeps_the_case_it_was_written_in() {
    let Filter::Present(attr) = parsed("userName pr") else {
        panic!("`userName pr` is a presence test");
    };
    assert_eq!(attr.attr, "userName",);
}

#[test]
fn a_urn_prefix_is_split_off_at_its_last_colon() {
    let Filter::Compare { attr, .. } =
        parsed(r#"urn:ietf:params:scim:schemas:core:2.0:User:userName sw "J""#)
    else {
        panic!("a fully qualified comparison");
    };
    assert_eq!(
        attr.urn.as_deref(),
        Some("urn:ietf:params:scim:schemas:core:2.0:User"),
    );
    assert_eq!(attr.attr, "userName");
    assert_eq!(attr.sub_attr, None);
}

#[test]
fn a_sub_attribute_is_kept_apart_from_its_parent() {
    let Filter::Compare { attr, .. } = parsed(r#"name.familyName co "O'Malley""#) else {
        panic!("a sub-attribute comparison");
    };
    assert_eq!(attr.attr, "name");
    assert_eq!(attr.sub_attr.as_deref(), Some("familyName"));
}

#[test]
fn a_comparison_value_may_be_any_json_literal() {
    for (filter, expected) in [
        (r#"a eq "s""#, CompareValue::String("s".to_owned())),
        ("a eq 42", CompareValue::Number(42.into())),
        (
            "a eq -1.5e3",
            CompareValue::Number(serde_json::from_str("-1.5e3").expect("a JSON number")),
        ),
        ("a eq true", CompareValue::Bool(true)),
        ("a eq false", CompareValue::Bool(false)),
        ("a eq null", CompareValue::Null),
    ] {
        let Filter::Compare { value, .. } = parsed(filter) else {
            panic!("`{filter}` is a comparison");
        };
        assert_eq!(value, expected, "the value of `{filter}`");
    }
}

#[test]
fn a_string_value_understands_the_json_escapes() {
    let Filter::Compare { value, .. } = parsed(r#"a eq "q\"b\\s\/n\nuäp😀""#) else {
        panic!("an escaped string comparison");
    };
    assert_eq!(value, CompareValue::String("q\"b\\s/n\nuäp😀".to_owned()),);
}

#[test]
fn whitespace_between_the_tokens_is_not_load_bearing() {
    for filter in [
        r#"userName eq"bjensen""#,
        r#"  userName    eq   "bjensen"  "#,
        r#"emails [ type eq "work" ]"#,
        "not(title pr)",
    ] {
        parse(filter).unwrap_or_else(|e| panic!("`{filter}` should be accepted, but: {e}"));
    }
}

#[test]
fn a_bare_value_path_is_a_filter_on_its_own() {
    let Filter::ValuePath { attr, filter } = parsed(r#"emails[type eq "work"]"#) else {
        panic!("a bare valuePath is one of FILTER's alternatives");
    };
    assert_eq!(attr, path("emails"));
    assert!(matches!(*filter, Filter::Compare { .. }));
}

#[test]
fn trailing_garbage_is_refused_rather_than_ignored() {
    let detail = rejected(r#"userName eq "bjensen" bogus"#);
    assert!(detail.contains("bogus"),);
}

#[test]
fn a_refused_filter_says_where_it_broke() {
    let detail = rejected(r#"userName qe "bjensen""#);
    assert!(detail.contains("character 9") && detail.contains("qe "),);
}

#[test]
fn a_refused_filter_is_a_400_with_an_invalid_filter_scim_type() {
    let error = parse("(((").expect_err("`(((` is not a filter");
    assert!(matches!(error, Error::InvalidFilter(_)), "got {error}");
    assert_eq!(error.status(), crate::StatusCode::BAD_REQUEST);
    assert_eq!(error.scim_type(), Some("invalidFilter"));
}

#[test]
fn a_filter_that_is_only_whitespace_is_refused() {
    rejected("");
    rejected("   ");
}

#[test]
fn an_unbalanced_group_is_refused() {
    rejected("(title pr");
    rejected("title pr)");
    rejected(r#"emails[type eq "work""#);
    rejected(r#"emails type eq "work"]"#);
}

#[test]
fn an_unknown_operator_is_refused() {
    rejected(r#"userName like "b""#);
    rejected(r#"userName "b""#);
}

#[test]
fn an_unterminated_string_is_refused() {
    rejected(r#"userName eq "bjensen"#);
    rejected(r#"userName eq "bjensen\""#);
}

#[test]
fn pr_takes_no_value() {
    rejected(r#"title pr "x""#);
}

#[test]
fn a_comparison_needs_a_value() {
    rejected("userName eq");
    rejected("userName eq bjensen");
}

#[test]
fn an_operator_run_into_the_attribute_name_is_refused() {
    rejected("titlepr");
    rejected(r#"userNameeq "b""#);
}

#[test]
fn an_attribute_named_after_an_operator_is_not_mistaken_for_one() {
    assert_eq!(parsed("principal pr"), present("principal"),);
    let Filter::Compare { value, .. } = parsed(r#"nickName co "and""#) else {
        panic!("a comparison whose value happens to spell a keyword");
    };
    assert_eq!(value, CompareValue::String("and".to_owned()));
    assert_eq!(parsed("not pr"), present("not"),);
}

#[test]
fn not_without_a_group_is_refused() {
    rejected(r#"not userName eq "b""#);
    rejected("not title pr");
}

#[test]
fn an_attribute_name_must_start_with_a_letter() {
    rejected("1title pr");
    rejected("name.1givenName pr");
}

#[test]
fn an_attribute_may_have_only_one_sub_attribute() {
    rejected("a.b.c pr");
    rejected("a. pr");
}

#[test]
fn a_value_path_may_not_be_followed_by_a_sub_attribute() {
    let detail = rejected(r#"emails[type eq "work"].value"#);
    assert!(detail.contains(".value"),);
}

#[test]
fn a_value_path_may_not_contain_another_value_path() {
    rejected(r#"emails[groups[value eq "x"]]"#);
}

#[test]
fn and_binds_tighter_than_or() {
    assert_eq!(
        parsed("a pr or b pr and c pr"),
        Filter::Or(
            Box::new(present("a")),
            Box::new(Filter::And(Box::new(present("b")), Box::new(present("c")))),
        ),
    );
}

#[test]
fn a_chain_of_the_same_operator_groups_to_the_left() {
    assert_eq!(
        parsed("a pr and b pr and c pr"),
        Filter::And(
            Box::new(Filter::And(Box::new(present("a")), Box::new(present("b")))),
            Box::new(present("c")),
        )
    );
}

#[test]
fn not_applies_only_to_its_own_group() {
    assert_eq!(
        parsed("not (a pr) and b pr"),
        Filter::And(
            Box::new(Filter::Not(Box::new(present("a")))),
            Box::new(present("b")),
        )
    );
}

#[test]
fn parentheses_override_the_default_precedence() {
    assert_eq!(
        parsed("(a pr or b pr) and c pr"),
        Filter::And(
            Box::new(Filter::Or(Box::new(present("a")), Box::new(present("b")))),
            Box::new(present("c")),
        )
    );
}

#[test]
fn every_filter_rfc_7644_prints_as_an_example_evaluates_against_a_user() {
    let user = bjensen();
    let expected = [
        true,  // userName eq "bjensen"
        true,  // name.familyName co "O'Malley"
        false, // userName sw "J"
        false, // urn:...:User:userName sw "J"
        true,  // title pr
        true,  // meta.lastModified gt "2011-05-13T04:42:34Z"
        true,  // meta.lastModified ge "2011-05-13T04:42:34Z"
        false, // meta.lastModified lt "2011-05-13T04:42:34Z"
        false, // meta.lastModified le "2011-05-13T04:42:34Z"
        true,  // title pr and userType eq "Employee"
        true,  // title pr or userType eq "Intern"
        true,  // schemas eq "urn:...:extension:enterprise:2.0:User"
        true,  // userType eq "Employee" and (emails co "example.com" or emails.value co "...org")
        false, // userType ne "Employee" and not (...)
        true,  // userType eq "Employee" and (emails.type eq "work")
        true,  // userType eq "Employee" and emails[type eq "work" and value co "@example.com"]
        true,  // emails[work and @example.com] or ims[xmpp and @foo.com]
    ];
    assert_eq!(RFC_EXAMPLES.len(), expected.len(), "one answer per example");
    for (filter, expected) in RFC_EXAMPLES.iter().zip(expected) {
        assert_eq!(matches(filter, &user), expected,);
    }
}

#[test]
fn a_string_comparison_ignores_case_by_default() {
    let user = bjensen();
    assert!(matches(r#"USERNAME eq "BJENSEN""#, &user),);
    assert!(matches(r#"userName co "JEN""#, &user));
    assert!(matches(r#"userName sw "BJ""#, &user));
    assert!(matches(r#"userName ew "SEN""#, &user));
}

#[test]
fn an_attribute_the_resource_does_not_carry_matches_nothing() {
    let user = bjensen();
    assert!(!matches(r#"costCenter eq "x""#, &user));
    assert!(!matches("costCenter pr", &user));
    assert!(matches(r#"costCenter ne "x""#, &user),);
}

#[test]
fn a_comparison_between_mismatched_types_does_not_match_rather_than_erroring() {
    let user = bjensen();
    assert!(!matches("userName eq 42", &user));
    assert!(!matches(r#"active eq "false""#, &user));
    assert!(!matches("active gt true", &user),);
}

#[test]
fn co_sw_and_ew_apply_to_strings_and_to_nothing_else() {
    let user = json!({ "count": 1234, "active": true });
    assert!(!matches("count co 23", &user));
    assert!(!matches("active sw true", &user));
}

#[test]
fn an_ordering_operator_compares_numbers_numerically_not_as_text() {
    let user = json!({ "count": 9 });
    assert!(matches("count lt 10", &user), "9 is less than 10");
    assert!(!matches("count gt 10", &user));
    assert!(matches("count eq 9.0", &user),);
}

#[test]
fn a_timestamp_orders_as_the_text_it_is_written_as() {
    let user = bjensen();
    assert!(matches(
        r#"meta.lastModified gt "2011-05-13T04:42:34Z""#,
        &user
    ));
    assert!(matches(
        r#"meta.lastModified lt "2013-01-01T00:00:00Z""#,
        &user
    ));
}

#[test]
fn pr_is_false_for_an_empty_or_absent_value() {
    let resource = json!({
        "empty": "",
        "nothing": null,
        "none": [],
        "blank": {},
        "off": false,
        "zero": 0,
    });
    for attr in ["empty", "nothing", "none", "blank", "missing"] {
        assert!(!matches(&format!("{attr} pr"), &resource),);
    }
    for attr in ["off", "zero"] {
        assert!(matches(&format!("{attr} pr"), &resource),);
    }
}

#[test]
fn a_multi_valued_attribute_matches_when_any_of_its_values_does() {
    let user = bjensen();
    assert!(matches(
        r#"schemas eq "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User""#,
        &user
    ));
    assert!(matches(r#"emails.type eq "home""#, &user));
    assert!(matches(r#"emails.value co "example.org""#, &user));
}

#[test]
fn a_multi_valued_complex_attribute_compares_against_its_value_sub_attribute() {
    let user = bjensen();
    assert!(matches(r#"emails co "example.com""#, &user),);
}

#[test]
fn a_value_path_needs_one_element_to_satisfy_every_condition() {
    let user = bjensen();
    assert!(!matches(
        r#"emails[type eq "work" and value co "example.org"]"#,
        &user
    ),);
    assert!(matches(
        r#"emails.type eq "work" and emails.value co "example.org""#,
        &user
    ),);
}

#[test]
fn an_extension_attribute_is_reached_through_its_urn() {
    let user = bjensen();
    assert!(matches(
        r#"urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:department eq "Tour Operations""#,
        &user
    ),);
    assert!(matches(
        r#"urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:employeeNumber eq "701984""#,
        &user
    ));
}

#[test]
fn a_core_attribute_may_be_written_with_its_urn_too() {
    let user = bjensen();
    assert!(matches(
        r#"urn:ietf:params:scim:schemas:core:2.0:User:userName eq "bjensen""#,
        &user
    ),);
}

#[test]
fn not_inverts_a_match_including_one_that_was_never_assigned() {
    let user = bjensen();
    assert!(matches("not (costCenter pr)", &user));
    assert!(!matches("not (userName pr)", &user));
}

#[test]
fn and_and_or_read_the_way_they_look() {
    let user = bjensen();
    assert!(matches(
        r#"userName eq "bjensen" or userName eq "jsmith""#,
        &user
    ));
    assert!(!matches(
        r#"userName eq "bjensen" and userName eq "jsmith""#,
        &user
    ));
}

#[test]
fn a_filter_matches_a_typed_resource_without_a_detour_through_json() {
    let group = crate::models::Group {
        schemas: vec![crate::models::Group::SCHEMA.to_owned()],
        display_name: "Tour Guides".to_owned(),
        ..Default::default()
    };
    assert!(
        parsed(r#"displayName eq "tour guides""#)
            .matches_resource(&group)
            .expect("a group serializes")
    );
}

#[test]
fn the_operator_enum_covers_every_keyword_the_rfc_defines() {
    for (keyword, expected) in [
        ("eq", CompareOp::Eq),
        ("ne", CompareOp::Ne),
        ("co", CompareOp::Co),
        ("sw", CompareOp::Sw),
        ("ew", CompareOp::Ew),
        ("gt", CompareOp::Gt),
        ("lt", CompareOp::Lt),
        ("ge", CompareOp::Ge),
        ("le", CompareOp::Le),
    ] {
        let Filter::Compare { op, .. } = parsed(&format!(r#"a {keyword} "x""#)) else {
            panic!("`a {keyword} \"x\"` is a comparison");
        };
        assert_eq!(op, expected);
    }
}

#[test]
fn an_attribute_path_renders_back_the_way_it_was_written() {
    for written in [
        "userName",
        "name.familyName",
        "urn:ietf:params:scim:schemas:core:2.0:User:userName",
        "urn:ietf:params:scim:schemas:extension:enterprise:2.0:User:manager.displayName",
    ] {
        let Filter::Present(attr) = parsed(&format!("{written} pr")) else {
            panic!("`{written} pr` is a presence test");
        };
        assert_eq!(attr.to_string(), written);
    }
}

#[test]
fn a_filter_longer_than_the_limit_is_refused_unread() {
    let long = "title pr or ".repeat(MAX_LENGTH) + "title pr";
    let detail = rejected(&long);
    assert!(detail.contains(&MAX_LENGTH.to_string()), "got {detail}");

    let chain = "title pr and ".repeat(MAX_LENGTH / 13 - 1) + "title pr";
    assert!(chain.len() <= MAX_LENGTH);
    assert!(matches(&chain, &bjensen()));
}

#[test]
fn a_filter_nested_deeper_than_the_limit_is_refused_rather_than_overflowing_the_stack() {
    let deep = "(".repeat(MAX_LENGTH / 2) + "title pr";
    let detail = rejected(&deep);
    assert!(
        detail.contains("nests more than") && detail.contains(&MAX_DEPTH.to_string()),
        "got {detail}"
    );

    let nested = format!("{}title pr{}", "(".repeat(MAX_DEPTH), ")".repeat(MAX_DEPTH));
    assert!(matches(&nested, &bjensen()), "{MAX_DEPTH} levels are fine");

    let one_too_many = format!(
        "{}title pr{}",
        "(".repeat(MAX_DEPTH + 1),
        ")".repeat(MAX_DEPTH + 1)
    );
    rejected(&one_too_many);
}

#[test]
fn a_value_path_counts_towards_the_nesting_limit_too() {
    let deep = format!("{}emails[type eq \"work\"]", "(".repeat(MAX_DEPTH));
    let detail = rejected(&deep);
    assert!(
        detail.contains("nests more than") && detail.contains(&MAX_DEPTH.to_string()),
        "got {detail}"
    );
}

#[test]
fn a_position_in_an_error_counts_characters_not_bytes() {
    let detail = rejected("displayName eq \"M\u{fc}nchen\" bogus");
    assert!(detail.contains("character 25"), "got {detail}");
}

#[test]
fn a_urn_the_resource_does_not_claim_matches_nothing() {
    let device = json!({
        "schemas": ["urn:example:params:scim:schemas:core:2.0:Device"],
        "location": "berlin",
    });
    assert!(matches(r#"location eq "berlin""#, &device));
    assert!(matches(
        r#"urn:example:params:scim:schemas:core:2.0:Device:location eq "berlin""#,
        &device
    ));
    assert!(!matches(
        r#"urn:example:params:scim:schemas:extension:site:2.0:Device:location eq "berlin""#,
        &device
    ));
}

#[test]
fn an_extension_attribute_wins_over_the_core_one_of_the_same_name() {
    let device = json!({
        "schemas": [
            "urn:example:params:scim:schemas:core:2.0:Device",
            "urn:example:params:scim:schemas:extension:site:2.0:Device",
        ],
        "location": "berlin",
        "urn:example:params:scim:schemas:extension:site:2.0:Device": { "location": "hamburg" },
    });
    assert!(matches(
        r#"urn:example:params:scim:schemas:extension:site:2.0:Device:location eq "hamburg""#,
        &device
    ));
    assert!(!matches(
        r#"urn:example:params:scim:schemas:extension:site:2.0:Device:location eq "berlin""#,
        &device
    ));
}

#[test]
fn a_resource_that_declares_no_schemas_reads_any_urn_at_its_root() {
    let hand_rolled = json!({ "userName": "bjensen" });
    assert!(matches(
        r#"urn:ietf:params:scim:schemas:core:2.0:User:userName eq "bjensen""#,
        &hand_rolled
    ));
}

#[test]
fn an_integer_past_a_doubles_precision_compares_exactly() {
    let resource = json!({ "count": 9_007_199_254_740_992_i64 });
    assert!(matches("count eq 9007199254740992", &resource));
    assert!(
        !matches("count eq 9007199254740993", &resource),
        "the two are distinct integers, however they round as `f64`"
    );
    assert!(matches("count lt 9007199254740993", &resource));
    assert!(!matches("count ge 9007199254740993", &resource));
}

#[test]
fn a_number_still_compares_across_the_integer_and_float_forms() {
    let resource = json!({ "count": 9, "ratio": 1.5 });
    assert!(matches("count eq 9.0", &resource));
    assert!(matches("count gt -1", &resource));
    assert!(matches("ratio lt 2", &resource));
    assert!(matches("ratio gt 1", &resource));
}

#[test]
fn a_non_ascii_value_still_folds_case_the_unicode_way() {
    let user = json!({ "displayName": "M\u{dc}NCHEN" });
    assert!(matches("displayName eq \"m\u{fc}nchen\"", &user));
    assert!(matches("displayName co \"\u{fc}nch\"", &user));
    assert!(matches("displayName sw \"m\u{fc}\"", &user));
    assert!(matches("displayName ew \"hen\"", &user));
}

fn users(names: &[&str]) -> Vec<crate::models::User> {
    names
        .iter()
        .map(|name| crate::models::User {
            user_name: (*name).to_owned(),
            ..Default::default()
        })
        .collect()
}

fn user_names(response: &crate::models::ListResponse<crate::models::User>) -> Vec<&str> {
    response
        .resources
        .iter()
        .map(|u| u.user_name.as_str())
        .collect()
}

#[test]
fn a_list_response_keeps_only_what_the_filter_matches() {
    let query = |filter: &str| crate::models::FilterQuery {
        filter: Some(filter.into()),
        ..Default::default()
    };

    let one = super::list_response(
        users(&["bjensen", "jsmith"]),
        &query(r#"userName eq "jsmith""#),
    )
    .expect("a list");
    assert_eq!(one.total_results, 1);
    assert_eq!(user_names(&one), ["jsmith"]);

    // Matching nothing is an empty page, not an error.
    let none = super::list_response(users(&["bjensen"]), &query(r#"userName eq "nobody""#))
        .expect("a list");
    assert_eq!(none.total_results, 0);
    assert!(none.resources.is_empty());
}

#[test]
fn a_list_response_pages_from_one_and_counts_every_match() {
    let query = crate::models::FilterQuery {
        start_index: Some(3),
        count: Some(2),
        ..Default::default()
    };
    let response =
        super::list_response(users(&["u1", "u2", "u3", "u4", "u5"]), &query).expect("a list");
    assert_eq!(response.total_results, 5, "the count before the page");
    assert_eq!(response.start_index, Some(3));
    assert_eq!(response.items_per_page, Some(2));
    assert_eq!(user_names(&response), ["u3", "u4"]);

    let from_zero = crate::models::FilterQuery {
        start_index: Some(0),
        ..Default::default()
    };
    let response = super::list_response(users(&["u1"]), &from_zero).expect("a list");
    assert_eq!(response.start_index, Some(1), "below 1 counts as 1");
    assert_eq!(user_names(&response), ["u1"]);
}

#[test]
fn a_list_response_refuses_a_filter_it_cannot_read() {
    let query = crate::models::FilterQuery {
        filter: Some("userName eq".into()),
        ..Default::default()
    };
    let error = super::list_response(users(&["bjensen"]), &query).unwrap_err();
    assert!(
        matches!(error, Error::InvalidFilter(_)),
        "answering the whole collection to a filter we could not read would look like success, \
         got {error:?}"
    );
}

const BADGE: &str = "urn:example:params:scim:schemas:core:2.0:Badge";
const BADGE_ISSUE: &str = "urn:example:params:scim:schemas:extension:issue:2.0:Badge";

/// A resource type whose schema declares `caseExact` on a core attribute, a sub-attribute and an
/// extension attribute, each next to one that does not.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
struct Badge(Value);

impl Resource for Badge {
    type Id = String;

    const RESOURCE_TYPE: &'static str = "Badge";
    const ENDPOINT: &'static str = "/Badges";
    const SCHEMA: &'static str = BADGE;

    fn id(&self) -> Option<&String> {
        None
    }

    fn meta(&self) -> Option<&crate::models::Meta> {
        None
    }

    fn set_meta(&mut self, _: crate::models::Meta) {}

    fn schema() -> crate::models::Schema {
        use crate::models::{Attributes, SubAttributes};
        crate::models::Schema::new(
            BADGE,
            "Badge",
            "A badge",
            vec![
                Attributes::new("code", "string").case_exact(),
                Attributes::new("label", "string"),
                Attributes::new("tags", "complex")
                    .multi_valued()
                    .sub_attributes(vec![
                        SubAttributes::new("value", "string").case_exact(),
                        SubAttributes::new("display", "string"),
                    ]),
            ],
        )
    }

    fn extensions() -> Vec<crate::models::Extension> {
        vec![crate::models::Extension::optional(
            crate::models::Schema::new(
                BADGE_ISSUE,
                "BadgeIssue",
                "Who issued a badge",
                vec![
                    crate::models::Attributes::new("serial", "string").case_exact(),
                    crate::models::Attributes::new("issuer", "string"),
                ],
            ),
        )]
    }
}

fn badge() -> Badge {
    Badge(json!({
        "schemas": [BADGE, BADGE_ISSUE],
        "code": "AbC",
        "label": "AbC",
        "tags": [{ "value": "Red", "display": "Red" }],
        (BADGE_ISSUE): { "serial": "XyZ", "issuer": "XyZ" },
    }))
}

fn matches_badge(filter: &str) -> bool {
    parsed(filter)
        .matches_resource(&badge())
        .expect("a badge serializes")
}

#[test]
fn a_case_exact_attribute_matches_only_in_its_own_case() {
    for (wrong, right) in [
        (r#"code eq "abc""#, r#"code eq "AbC""#),
        (r#"code co "B""#, r#"code co "b""#),
        (r#"code sw "a""#, r#"code sw "A""#),
        (r#"code ew "c""#, r#"code ew "C""#),
    ] {
        assert!(!matches_badge(wrong), "`{wrong}` differs in case");
        assert!(matches_badge(right), "`{right}` does not");
        let folded = wrong.replace("code", "LABEL");
        assert!(matches_badge(&folded), "`{folded}` is not caseExact");
    }
    assert!(matches_badge(r#"code ne "abc""#));
}

#[test]
fn case_exact_is_read_from_sub_attributes_and_extensions_too() {
    for (wrong, matching) in [
        (r#"tags.value eq "red""#, r#"tags.display eq "red""#),
        (r#"tags[value eq "red"]"#, r#"tags[display eq "red"]"#),
        // A comparison on the complex attribute itself falls through to its `value`.
        (r#"tags eq "red""#, r#"tags eq "Red""#),
        (
            r#"urn:example:params:scim:schemas:extension:issue:2.0:Badge:serial eq "xyz""#,
            r#"urn:example:params:scim:schemas:extension:issue:2.0:Badge:issuer eq "xyz""#,
        ),
    ] {
        assert!(!matches_badge(wrong), "`{wrong}` differs in case");
        assert!(matches_badge(matching), "`{matching}` should match");
    }
}

#[test]
fn the_built_in_schemas_make_id_and_external_id_case_exact_but_not_user_name() {
    let user = crate::models::User {
        id: Some("Id-1".into()),
        external_id: Some("Ext-1".into()),
        user_name: "bjensen".into(),
        meta: Some(crate::models::Meta {
            resource_type: Some("User".into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let matches_user = |filter: &str| parsed(filter).matches_resource(&user).expect("a user");

    assert!(!matches_user(r#"id eq "id-1""#));
    assert!(!matches_user(r#"externalId eq "ext-1""#));
    assert!(!matches_user(r#"meta.resourceType eq "user""#));
    assert!(
        matches_user(r#"ID eq "Id-1""#),
        "attribute names still fold"
    );
    assert!(matches_user(r#"userName eq "BJensen""#));
}

#[test]
fn a_patch_names_its_values_through_the_path_or_the_value() {
    let ids = |op: Operation| patch_values(&op);
    assert_eq!(
        ids(Operation::remove(r#"members[value eq "7"]"#.into())),
        ["7"]
    );
    assert_eq!(
        ids(Operation::remove(
            r#"urn:ietf:params:scim:schemas:core:2.0:Group:members[type eq "User" and value eq "7"]"#
                .into()
        )),
        ["7"],
        "the bracket is filter grammar, so a compound one still names its member"
    );
    assert_eq!(
        ids(Operation::remove(r#"members[not (value eq "7")]"#.into())),
        Vec::<String>::new(),
        "a negation names everyone but 7, which is not a value to act on"
    );
    assert_eq!(
        ids(Operation::add(
            "members".into(),
            Some(json!([{"value": "7"}, "9", {"value": ""}]))
        )),
        ["7", "9"]
    );
}
