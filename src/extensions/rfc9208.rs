//! IMAP QUOTA Extension

use std::convert::TryFrom;

use abnf_core::streaming::SP;
use imap_types::{
    command::CommandBody,
    core::{AString, NonEmptyVec},
    extensions::rfc9208::{QuotaGet, QuotaSet, Resource},
    response::{data::Capability, Data},
};
use nom::{
    bytes::{complete::tag, streaming::tag_no_case},
    combinator::map,
    multi::{many0, separated_list0, separated_list1},
    sequence::{delimited, preceded, tuple},
    IResult,
};

use crate::rfc3501::{
    core::{astring, atom, number64},
    mailbox::mailbox,
};

/// ```abnf
/// quota-root-name = astring
/// ```
#[inline]
pub fn quota_root_name(input: &[u8]) -> IResult<&[u8], AString> {
    astring(input)
}

/// ```abnf
/// getquota = "GETQUOTA" SP quota-root-name
/// ```
#[inline]
pub fn getquota(input: &[u8]) -> IResult<&[u8], CommandBody> {
    let mut parser = tuple((tag_no_case("GETQUOTA "), quota_root_name));

    let (remaining, (_, root)) = parser(input)?;

    Ok((remaining, CommandBody::GetQuota { root }))
}

/// ```abnf
/// getquotaroot = "GETQUOTAROOT" SP mailbox
/// ```
pub fn getquotaroot(input: &[u8]) -> IResult<&[u8], CommandBody> {
    let mut parser = tuple((tag_no_case("GETQUOTAROOT "), mailbox));

    let (remaining, (_, mailbox)) = parser(input)?;

    Ok((remaining, CommandBody::GetQuotaRoot { mailbox }))
}

/// ```abnf
/// quota-resource = resource-name SP
///                  resource-usage SP
///                  resource-limit
///
/// resource-usage = number64
///
/// resource-limit = number64
/// ```
pub fn quota_resource(input: &[u8]) -> IResult<&[u8], QuotaGet> {
    let mut parser = tuple((resource_name, SP, number64, SP, number64));

    let (remaining, (resource, _, usage, _, limit)) = parser(input)?;

    Ok((
        remaining,
        QuotaGet {
            resource,
            usage,
            limit,
        },
    ))
}

/// ```abnf
/// resource-name = "STORAGE" /
///                 "MESSAGE" /
///                 "MAILBOX" /
///                 "ANNOTATION-STORAGE" /
///                 resource-name-ext
///
/// resource-name-ext = atom
/// ```
pub fn resource_name(input: &[u8]) -> IResult<&[u8], Resource> {
    map(atom, Resource::from)(input)
}

/// ```abnf
/// quota-response = "QUOTA" SP quota-root-name SP quota-list
///
/// quota-list = "(" quota-resource *(SP quota-resource) ")"
/// ```
pub fn quota_response(input: &[u8]) -> IResult<&[u8], Data> {
    let mut parser = tuple((
        tag_no_case("QUOTA "),
        quota_root_name,
        SP,
        delimited(tag("("), separated_list1(SP, quota_resource), tag(")")),
    ));

    let (remaining, (_, root, _, quotas)) = parser(input)?;

    Ok((
        remaining,
        Data::Quota {
            root,
            // Safety: Safe because we use `separated_list1` above.
            quotas: NonEmptyVec::try_from(quotas).unwrap(),
        },
    ))
}

/// ```abnf
/// quotaroot-response = "QUOTAROOT" SP mailbox *(SP quota-root-name)
/// ```
pub fn quotaroot_response(input: &[u8]) -> IResult<&[u8], Data> {
    let mut parser = tuple((
        tag_no_case("QUOTAROOT "),
        mailbox,
        many0(preceded(SP, quota_root_name)),
    ));

    let (remaining, (_, mailbox, roots)) = parser(input)?;

    Ok((remaining, Data::QuotaRoot { mailbox, roots }))
}

/// ```abnf
/// setquota = "SETQUOTA" SP quota-root-name SP setquota-list
///
/// setquota-list = "(" [setquota-resource *(SP setquota-resource)] ")"
/// ```
pub fn setquota(input: &[u8]) -> IResult<&[u8], CommandBody> {
    let mut parser = tuple((
        tag_no_case("SETQUOTA "),
        quota_root_name,
        SP,
        delimited(tag("("), separated_list0(SP, setquota_resource), tag(")")),
    ));

    let (remaining, (_, root, _, quotas)) = parser(input)?;

    Ok((remaining, CommandBody::SetQuota { root, quotas }))
}

/// ```abnf
/// setquota-resource = resource-name SP resource-limit
/// ```
pub fn setquota_resource(input: &[u8]) -> IResult<&[u8], QuotaSet> {
    let mut parser = tuple((resource_name, SP, number64));

    let (remaining, (resource, _, limit)) = parser(input)?;

    Ok((remaining, QuotaSet { resource, limit }))
}

// This had to be inlined into the `capability` parser because `CapabilityOther("QUOTAFOO")` would
// be parsed as `Capability::Quota` plus an erroneous remainder. The `capability` parser eagerly consumes
// an `atom` and tries to detect the variants later.
// /// ```abnf
// /// capability-quota = "QUOTASET" / capa-quota-res
// /// ```
// ///
// /// Note: Extended to ...
// ///
// /// ```abnf
// /// capability-quota = "QUOTASET" / capa-quota-res / "QUOTA"
// /// ```
// pub fn capability_quota(input: &[u8]) -> IResult<&[u8], Capability> {
//     alt((
//         value(Capability::QuotaSet, tag_no_case("QUOTASET")),
//         capa_quota_res,
//         value(Capability::Quota, tag_no_case("QUOTA")),
//     ))(input)
// }

/// ```abnf
/// capa-quota-res = "QUOTA=RES-" resource-name
/// ```
pub fn capa_quota_res(input: &[u8]) -> IResult<&[u8], Capability> {
    let mut parser = preceded(tag_no_case("QUOTA=RES-"), resource_name);

    let (remaining, resource) = parser(input)?;

    Ok((remaining, Capability::QuotaRes(resource)))
}

#[cfg(test)]
mod test {
    use super::*;
    use crate::imap_types::extensions::rfc9208::ResourceOther;

    #[test]
    fn test_resource() {
        let tests = [
            (b"stOragE ".as_ref(), Resource::Storage),
            (b"mesSaGe ".as_ref(), Resource::Message),
            (b"maIlbOx ".as_ref(), Resource::Mailbox),
            (b"anNotatIon-stoRage ".as_ref(), Resource::AnnotationStorage),
            (
                b"anNotatIon-stoRageX ".as_ref(),
                Resource::Other(ResourceOther::try_from(b"anNotatIon-stoRageX".as_ref()).unwrap()),
            ),
            (
                b"anNotatIon-stoRagee ".as_ref(),
                Resource::Other(ResourceOther::try_from(b"anNotatIon-stoRagee".as_ref()).unwrap()),
            ),
        ];

        for (test, expected) in tests.iter() {
            let (rem, got) = resource_name(test).unwrap();
            assert_eq!(*expected, got);
            assert_eq!(rem, b" ");
        }
    }
}
