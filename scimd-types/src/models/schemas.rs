//! The schema definitions for the two resource types RFC 7643 predefines, transcribed from
//! sections 8.7.1 and 8.7.2, and the common attributes of section 3.1 that neither lists. Attribute
//! defaults come from [`Attributes::new`], so what is spelled out here is only what the RFC states
//! explicitly beyond them.

use super::schema::{Attributes, Schema, SubAttributes};
use crate::Resource as _;

const DISPLAY_DESCRIPTION: &str = "A human-readable name, primarily used for display purposes.";

fn value_display_type_primary(
    value: SubAttributes,
    type_description: &str,
    canonical_values: &[&str],
    primary_description: &str,
) -> Vec<SubAttributes> {
    let mut r#type = SubAttributes::new("type", "string").description(type_description);
    if !canonical_values.is_empty() {
        r#type = r#type.canonical_values(canonical_values);
    }

    vec![
        value,
        SubAttributes::new("display", "string").description(DISPLAY_DESCRIPTION),
        r#type,
        SubAttributes::new("primary", "boolean").description(primary_description),
    ]
}

fn unlabelled_value_display_type_primary(value: SubAttributes) -> Vec<SubAttributes> {
    value_display_type_primary(
        value,
        "A label indicating the attribute's function.",
        &[],
        "A Boolean value indicating the 'primary' or preferred attribute value for this attribute. \
         The primary attribute value 'true' MUST appear no more than once.",
    )
}

impl Schema {
    /// The attributes RFC 7643 section 3.1 gives every resource. No schema lists them, but their
    /// characteristics still apply: `id`, `externalId`, `meta.resourceType` and `meta.version` are
    /// `caseExact`.
    pub fn common_attributes() -> Vec<Attributes> {
        vec![
            Attributes::new("id", "string")
                .case_exact()
                .mutability("readOnly")
                .returned("always"),
            Attributes::new("externalId", "string").case_exact(),
            Attributes::new("meta", "complex")
                .mutability("readOnly")
                .sub_attributes(vec![
                    SubAttributes::new("resourceType", "string")
                        .case_exact()
                        .mutability("readOnly"),
                    SubAttributes::new("created", "dateTime").mutability("readOnly"),
                    SubAttributes::new("lastModified", "dateTime").mutability("readOnly"),
                    SubAttributes::new("location", "reference").mutability("readOnly"),
                    SubAttributes::new("version", "string")
                        .case_exact()
                        .mutability("readOnly"),
                ]),
        ]
    }
}

pub(super) fn user() -> Schema {
    Schema::new(
        super::User::SCHEMA,
        "User",
        "User Account",
        vec![
            Attributes::new("userName", "string")
                .description(
                    "Unique identifier for the User, typically used by the user to directly \
                     authenticate to the service provider. Each User MUST include a non-empty \
                     userName value. This identifier MUST be unique across the service provider's \
                     entire set of Users.",
                )
                .required()
                .uniqueness("server"),
            Attributes::new("name", "complex")
                .description(
                    "The components of the user's real name. Providers MAY return just the full \
                     name as a single string in the formatted sub-attribute, or they MAY return \
                     just the individual component attributes using the other sub-attributes, or \
                     they MAY return both.",
                )
                .sub_attributes(vec![
                    SubAttributes::new("formatted", "string").description(
                        "The full name, including all middle names, titles, and suffixes as \
                         appropriate, formatted for display.",
                    ),
                    SubAttributes::new("familyName", "string").description(
                        "The family name of the User, or last name in most Western languages.",
                    ),
                    SubAttributes::new("givenName", "string").description(
                        "The given name of the User, or first name in most Western languages.",
                    ),
                    SubAttributes::new("middleName", "string")
                        .description("The middle name(s) of the User."),
                    SubAttributes::new("honorificPrefix", "string").description(
                        "The honorific prefix(es) of the User, or title in most Western languages.",
                    ),
                    SubAttributes::new("honorificSuffix", "string").description(
                        "The honorific suffix(es) of the User, or suffix in most Western \
                         languages.",
                    ),
                ]),
            Attributes::new("displayName", "string").description(
                "The name of the User, suitable for display to end-users. The name SHOULD be the \
                 full name of the User being described, if known.",
            ),
            Attributes::new("nickName", "string").description(
                "The casual way to address the user in real life, e.g., 'Bob' or 'Bobby' instead \
                 of 'Robert'.",
            ),
            Attributes::new("profileUrl", "reference")
                .description(
                    "A fully qualified URL pointing to a page representing the User's online \
                     profile.",
                )
                .reference_types(&["external"]),
            Attributes::new("title", "string")
                .description("The user's title, such as 'Vice President'."),
            Attributes::new("userType", "string").description(
                "Used to identify the relationship between the organization and the user. Typical \
                 values used might be 'Contractor', 'Employee', 'Intern', 'Temp', 'External', and \
                 'Unknown', but any value may be used.",
            ),
            Attributes::new("preferredLanguage", "string").description(
                "Indicates the User's preferred written or spoken language. Generally used for \
                 selecting a localized user interface; e.g., 'en_US' specifies the language \
                 English and country US.",
            ),
            Attributes::new("locale", "string").description(
                "Used to indicate the User's default location for purposes of localizing items \
                 such as currency, date time format, or numerical representations.",
            ),
            Attributes::new("timezone", "string").description(
                "The User's time zone in the 'Olson' time zone database format, e.g., \
                 'America/Los_Angeles'.",
            ),
            Attributes::new("active", "boolean")
                .description("A Boolean value indicating the User's administrative status."),
            Attributes::new("password", "string")
                .description(
                    "The User's cleartext password. This attribute is intended to be used as a \
                     means to specify an initial password when creating a new User or to reset an \
                     existing User's password.",
                )
                .mutability("writeOnly")
                .returned("never"),
            Attributes::new("emails", "complex")
                .description(
                    "Email addresses for the user. The value SHOULD be canonicalized by the \
                     service provider, e.g., 'bjensen@example.com' instead of \
                     'bjensen@EXAMPLE.COM'. Canonical type values of 'work', 'home', and 'other'.",
                )
                .multi_valued()
                .sub_attributes(value_display_type_primary(
                    SubAttributes::new("value", "string").description(
                        "Email addresses for the user. The value SHOULD be canonicalized by the \
                         service provider, e.g., 'bjensen@example.com' instead of \
                         'bjensen@EXAMPLE.COM'.",
                    ),
                    "A label indicating the attribute's function, e.g., 'work' or 'home'.",
                    &["work", "home", "other"],
                    "A Boolean value indicating the 'primary' or preferred attribute value for \
                     this attribute, e.g., the preferred mailing address or primary email address. \
                     The primary attribute value 'true' MUST appear no more than once.",
                )),
            Attributes::new("phoneNumbers", "complex")
                .description(
                    "Phone numbers for the User. The value SHOULD be canonicalized by the service \
                     provider according to the format specified in RFC 3966, e.g., \
                     'tel:+1-201-555-0123'. Canonical type values of 'work', 'home', 'mobile', \
                     'fax', 'pager', and 'other'.",
                )
                .multi_valued()
                .sub_attributes(value_display_type_primary(
                    SubAttributes::new("value", "string").description("Phone number of the User."),
                    "A label indicating the attribute's function, e.g., 'work', 'home', 'mobile'.",
                    &["work", "home", "mobile", "fax", "pager", "other"],
                    "A Boolean value indicating the 'primary' or preferred attribute value for \
                     this attribute, e.g., the preferred phone number or primary phone number. The \
                     primary attribute value 'true' MUST appear no more than once.",
                )),
            Attributes::new("ims", "complex")
                .description("Instant messaging addresses for the User.")
                .multi_valued()
                .sub_attributes(value_display_type_primary(
                    SubAttributes::new("value", "string")
                        .description("Instant messaging address for the User."),
                    "A label indicating the attribute's function, e.g., 'aim', 'gtalk', 'xmpp'.",
                    &["aim", "gtalk", "icq", "xmpp", "msn", "skype", "qq", "yahoo"],
                    "A Boolean value indicating the 'primary' or preferred attribute value for \
                     this attribute, e.g., the preferred messenger or primary messenger. The \
                     primary attribute value 'true' MUST appear no more than once.",
                )),
            Attributes::new("photos", "complex")
                .description("URLs of photos of the User.")
                .multi_valued()
                .sub_attributes(value_display_type_primary(
                    SubAttributes::new("value", "reference")
                        .description("URL of a photo of the User.")
                        .reference_types(&["external"]),
                    "A label indicating the attribute's function, i.e., 'photo' or 'thumbnail'.",
                    &["photo", "thumbnail"],
                    "A Boolean value indicating the 'primary' or preferred attribute value for \
                     this attribute, e.g., the preferred photo or thumbnail. The primary attribute \
                     value 'true' MUST appear no more than once.",
                )),
            Attributes::new("addresses", "complex")
                .description(
                    "A physical mailing address for this User. Canonical type values of 'work', \
                     'home', and 'other'. This attribute is a complex type with the following \
                     sub-attributes.",
                )
                .multi_valued()
                .sub_attributes(vec![
                    SubAttributes::new("formatted", "string").description(
                        "The full mailing address, formatted for display or use with a mailing \
                         label. This attribute MAY contain newlines.",
                    ),
                    SubAttributes::new("streetAddress", "string").description(
                        "The full street address component, which may include house number, \
                         street name, P.O. box, and multi-line extended street address \
                         information. This attribute MAY contain newlines.",
                    ),
                    SubAttributes::new("locality", "string")
                        .description("The city or locality component."),
                    SubAttributes::new("region", "string")
                        .description("The state or region component."),
                    SubAttributes::new("postalCode", "string")
                        .description("The zip code or postal code component."),
                    SubAttributes::new("country", "string")
                        .description("The country name component."),
                    SubAttributes::new("type", "string")
                        .description(
                            "A label indicating the attribute's function, e.g., 'work' or 'home'.",
                        )
                        .canonical_values(&["work", "home", "other"]),
                ]),
            Attributes::new("groups", "complex")
                .description(
                    "A list of groups to which the user belongs, either through direct \
                     membership, through nested groups, or dynamically calculated.",
                )
                .multi_valued()
                .mutability("readOnly")
                .sub_attributes(vec![
                    SubAttributes::new("value", "string")
                        .description("The identifier of the User's group.")
                        .mutability("readOnly"),
                    SubAttributes::new("$ref", "reference")
                        .description(
                            "The URI of the corresponding 'Group' resource to which the user \
                             belongs.",
                        )
                        .reference_types(&["User", "Group"])
                        .mutability("readOnly"),
                    SubAttributes::new("display", "string")
                        .description(DISPLAY_DESCRIPTION)
                        .mutability("readOnly"),
                    SubAttributes::new("type", "string")
                        .description(
                            "A label indicating the attribute's function, e.g., 'direct' or \
                             'indirect'.",
                        )
                        .canonical_values(&["direct", "indirect"])
                        .mutability("readOnly"),
                ]),
            Attributes::new("entitlements", "complex")
                .description(
                    "A list of entitlements for the User that represent a thing the User has.",
                )
                .multi_valued()
                .sub_attributes(unlabelled_value_display_type_primary(
                    SubAttributes::new("value", "string")
                        .description("The value of an entitlement."),
                )),
            Attributes::new("roles", "complex")
                .description(
                    "A list of roles for the User that collectively represent who the User is, \
                     e.g., 'Student', 'Faculty'.",
                )
                .multi_valued()
                .sub_attributes(unlabelled_value_display_type_primary(
                    SubAttributes::new("value", "string").description("The value of a role."),
                )),
            Attributes::new("x509Certificates", "complex")
                .description("A list of certificates issued to the User.")
                .multi_valued()
                .sub_attributes(unlabelled_value_display_type_primary(
                    SubAttributes::new("value", "binary")
                        .description("The value of an X.509 certificate."),
                )),
        ],
    )
}

pub(super) fn enterprise_user() -> Schema {
    Schema::new(
        super::enterprise_user::SCHEMA,
        "EnterpriseUser",
        "Enterprise User",
        vec![
            Attributes::new("employeeNumber", "string").description(
                "Numeric or alphanumeric identifier assigned to a person, typically based on \
                 order of hire or association with an organization.",
            ),
            Attributes::new("costCenter", "string")
                .description("Identifies the name of a cost center."),
            Attributes::new("organization", "string")
                .description("Identifies the name of an organization."),
            Attributes::new("division", "string").description("Identifies the name of a division."),
            Attributes::new("department", "string")
                .description("Identifies the name of a department."),
            Attributes::new("manager", "complex")
                .description(
                    "The User's manager. A complex type that optionally allows service providers \
                     to represent organizational hierarchy by referencing the 'id' attribute of \
                     another User.",
                )
                .sub_attributes(vec![
                    SubAttributes::new("value", "string").description(
                        "The id of the SCIM resource representing the User's manager. REQUIRED.",
                    ),
                    SubAttributes::new("$ref", "reference")
                        .description(
                            "The URI of the SCIM resource representing the User's manager. \
                             REQUIRED.",
                        )
                        .reference_types(&["User"]),
                    SubAttributes::new("displayName", "string")
                        .description(
                            "The displayName of the User's manager. OPTIONAL and READ-ONLY.",
                        )
                        .mutability("readOnly"),
                ]),
        ],
    )
}

pub(super) fn group() -> Schema {
    Schema::new(
        super::Group::SCHEMA,
        "Group",
        "Group",
        vec![
            Attributes::new("displayName", "string")
                .description("A human-readable name for the Group. REQUIRED.")
                .required(),
            Attributes::new("members", "complex")
                .description("A list of members of the Group.")
                .multi_valued()
                .sub_attributes(vec![
                    SubAttributes::new("value", "string")
                        .description("Identifier of the member of this Group.")
                        .mutability("immutable"),
                    SubAttributes::new("$ref", "reference")
                        .description(
                            "The URI corresponding to a SCIM resource that is a member of this \
                             Group.",
                        )
                        .reference_types(&["User", "Group"])
                        .mutability("immutable"),
                    SubAttributes::new("type", "string")
                        .description(
                            "A label indicating the type of resource, e.g., 'User' or 'Group'.",
                        )
                        .canonical_values(&["User", "Group"])
                        .mutability("immutable"),
                    SubAttributes::new("display", "string")
                        .description(DISPLAY_DESCRIPTION)
                        .mutability("immutable"),
                ]),
        ],
    )
}
