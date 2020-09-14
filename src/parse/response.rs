use crate::{
    parse::{
        auth_type,
        core::{atom, base64, charset, is_text_char, nz_number, tag_imap, text},
        flag::flag_perm,
        mailbox::mailbox_data,
        message::message_data,
    },
    types::{
        core::txt,
        response::{Capability, Code, Continuation, Data, Response, Status},
    },
};
use abnf_core::streaming::{CRLF_relaxed as CRLF, SP};
use nom::{
    branch::alt,
    bytes::streaming::{tag, tag_no_case, take_while1},
    combinator::{map, map_res, opt, value},
    multi::separated_nonempty_list,
    sequence::{delimited, preceded, terminated, tuple},
    IResult,
};
use std::str::from_utf8;

// ----- greeting -----

/// greeting = "*" SP (resp-cond-auth / resp-cond-bye) CRLF
pub fn greeting(input: &[u8]) -> IResult<&[u8], Response> {
    let parser = tuple((
        tag(b"*"),
        SP,
        alt((
            map(
                resp_cond_auth,
                |(raw_status, (maybe_code, comment))| match raw_status.to_lowercase().as_ref() {
                    "ok" => Status::Ok {
                        tag: None,
                        code: maybe_code,
                        text: comment.to_owned(),
                    },
                    "preauth" => Status::PreAuth {
                        code: maybe_code,
                        text: comment.to_owned(),
                    },
                    _ => unreachable!(),
                },
            ),
            map(resp_cond_bye, |(maybe_code, comment)| Status::Bye {
                code: maybe_code,
                text: comment.to_owned(),
            }),
        )),
        CRLF,
    ));

    let (remaining, (_, _, status, _)) = parser(input)?;

    Ok((remaining, Response::Status(status)))
}

/// Authentication condition
///
/// resp-cond-auth = ("OK" / "PREAUTH") SP resp-text
fn resp_cond_auth(input: &[u8]) -> IResult<&[u8], (&str, (Option<Code>, txt))> {
    let parser = tuple((
        map_res(
            alt((tag_no_case(b"OK"), tag_no_case(b"PREAUTH"))),
            from_utf8, // FIXME(perf): use from_utf8_unchecked
        ),
        SP,
        resp_text,
    ));

    let (remaining, (raw_status, _, resp_text)) = parser(input)?;

    Ok((remaining, (raw_status, resp_text)))
}

/// resp-text = ["[" resp-text-code "]" SP] text
fn resp_text(input: &[u8]) -> IResult<&[u8], (Option<Code>, txt)> {
    tuple((
        opt(terminated(
            delimited(tag(b"["), resp_text_code, tag(b"]")),
            SP,
        )),
        text,
    ))(input)
}

/// ; errata id: 261
/// resp-text-code = "ALERT" /
///                  "BADCHARSET" [SP "(" charset *(SP charset) ")" ] /
///                  capability-data /
///                  "PARSE" /
///                  "PERMANENTFLAGS" SP "(" [flag-perm *(SP flag-perm)] ")" /
///                  "READ-ONLY" /
///                  "READ-WRITE" /
///                  "TRYCREATE" /
///                  "UIDNEXT" SP nz-number /
///                  "UIDVALIDITY" SP nz-number /
///                  "UNSEEN" SP nz-number /
///                  atom [SP 1*<any TEXT-CHAR except "]">]
fn resp_text_code(input: &[u8]) -> IResult<&[u8], Code> {
    alt((
        value(Code::Alert, tag_no_case(b"ALERT")),
        map(
            tuple((
                tag_no_case(b"BADCHARSET"),
                opt(preceded(
                    SP,
                    delimited(tag(b"("), separated_nonempty_list(SP, charset), tag(b")")),
                )),
            )),
            |(_, maybe_charsets)| Code::BadCharset(maybe_charsets.unwrap_or_default()),
        ),
        map(capability_data, Code::Capability),
        value(Code::Parse, tag_no_case(b"PARSE")),
        map(
            tuple((
                tag_no_case(b"PERMANENTFLAGS"),
                SP,
                delimited(
                    tag(b"("),
                    map(opt(separated_nonempty_list(SP, flag_perm)), |maybe_flags| {
                        maybe_flags.unwrap_or_default()
                    }),
                    tag(b")"),
                ),
            )),
            |(_, _, flags)| Code::PermanentFlags(flags),
        ),
        value(Code::ReadOnly, tag_no_case(b"READ-ONLY")),
        value(Code::ReadWrite, tag_no_case(b"READ-WRITE")),
        value(Code::TryCreate, tag_no_case(b"TRYCREATE")),
        map(
            tuple((tag_no_case(b"UIDNEXT"), SP, nz_number)),
            |(_, _, num)| Code::UidNext(num),
        ),
        map(
            tuple((tag_no_case(b"UIDVALIDITY"), SP, nz_number)),
            |(_, _, num)| Code::UidValidity(num),
        ),
        map(
            tuple((tag_no_case(b"UNSEEN"), SP, nz_number)),
            |(_, _, num)| Code::Unseen(num),
        ),
        map(
            tuple((
                atom,
                opt(preceded(
                    SP,
                    map_res(
                        take_while1(|byte| is_text_char(byte) && byte != b'"'),
                        from_utf8, // FIXME(perf): use from_utf8_unchecked
                    ),
                )),
            )),
            |(atom, maybe_params)| {
                Code::Other(atom.to_owned(), maybe_params.map(|inner| inner.to_owned()))
            },
        ),
    ))(input)
}

/// capability-data = "CAPABILITY" *(SP capability) SP "IMAP4rev1" *(SP capability)
///
/// Servers MUST implement the STARTTLS, AUTH=PLAIN, and LOGINDISABLED capabilities
/// Servers which offer RFC 1730 compatibility MUST list "IMAP4" as the first capability.
fn capability_data(input: &[u8]) -> IResult<&[u8], Vec<Capability>> {
    let parser = tuple((
        tag_no_case("CAPABILITY"),
        SP,
        separated_nonempty_list(SP, capability),
    ));

    let (rem, (_, _, caps)) = parser(input)?;

    if !caps.contains(&Capability::Imap4Rev1) {
        return Err(nom::Err::Error(nom::error::make_error(
            input,
            nom::error::ErrorKind::Verify, // TODO(verify): use `Failure` or `Error`?
        )));
    }

    Ok((rem, caps))
}

/// capability = ("AUTH=" auth-type) / atom
fn capability(input: &[u8]) -> IResult<&[u8], Capability> {
    alt((
        map(
            tuple((tag_no_case(b"AUTH="), auth_type)),
            |(_, mechanism)| Capability::Auth(mechanism),
        ),
        map(atom, |atom| {
            match atom.0.to_lowercase().as_ref() {
                "imap4rev1" => Capability::Imap4Rev1,
                "starttls" => Capability::StartTls,
                // RFC 2177 IMAP4 IDLE command
                "idle" => Capability::Idle,
                // RFC 2193 IMAP4 Mailbox Referrals
                "mailbox-referrals" => Capability::MailboxReferrals,
                // RFC 2221 IMAP4 Login Referrals
                "login-referrals" => Capability::LoginReferrals,
                // RFC 4959 IMAP Extension for SASL Initial Client Response
                "sasl-ir" => Capability::SaslIr,
                // RFC 5161 The IMAP ENABLE Extension
                "enable" => Capability::Enable,
                _ => Capability::Other(atom.to_owned()),
            }
        }),
    ))(input)
}

/// resp-cond-bye = "BYE" SP resp-text
fn resp_cond_bye(input: &[u8]) -> IResult<&[u8], (Option<Code>, txt)> {
    let parser = tuple((tag_no_case(b"BYE"), SP, resp_text));

    let (remaining, (_, _, resp_text)) = parser(input)?;

    Ok((remaining, resp_text))
}

// ----- response -----

/// response = *(continue-req / response-data) response-done
pub fn response(input: &[u8]) -> IResult<&[u8], Response> {
    // Divert from standard here for better usability.
    // response_data already contains the bye response, thus
    // response_done could also be response_tagged.
    //
    // However, I will keep it as it is for now.
    alt((
        map(continue_req, Response::Continuation),
        response_data,
        map(response_done, Response::Status),
    ))(input)
}

/// continue-req = "+" SP (resp-text / base64) CRLF
fn continue_req(input: &[u8]) -> IResult<&[u8], Continuation> {
    let parser = tuple((
        tag(b"+"),
        SP,
        alt((
            map(resp_text, |(code, text)| Continuation::Basic {
                code,
                text: text.to_owned(),
            }),
            map(base64, |str| Continuation::Base64(str.to_owned())),
        )),
        CRLF,
    ));

    let (remaining, (_, _, continuation, _)) = parser(input)?;

    Ok((remaining, continuation))
}

/// response-data = "*" SP (
///                 resp-cond-state /
///                 resp-cond-bye /
///                 mailbox-data /
///                 message-data /
///                 capability-data
///                 ) CRLF
fn response_data(input: &[u8]) -> IResult<&[u8], Response> {
    let parser = tuple((
        tag(b"*"),
        SP,
        alt((
            map(resp_cond_state, |(raw_status, code, text)| {
                let status = match raw_status.to_lowercase().as_ref() {
                    "ok" => Status::Ok {
                        tag: None,
                        code,
                        text: text.to_owned(),
                    },
                    "no" => Status::No {
                        tag: None,
                        code,
                        text: text.to_owned(),
                    },
                    "bad" => Status::Bad {
                        tag: None,
                        code,
                        text: text.to_owned(),
                    },
                    _ => unreachable!(),
                };

                Response::Status(status)
            }),
            map(resp_cond_bye, |(code, text)| {
                Response::Status({
                    Status::Bye {
                        code,
                        text: text.to_owned(),
                    }
                })
            }),
            map(mailbox_data, Response::Data),
            map(message_data, Response::Data),
            map(capability_data, |caps| {
                Response::Data(Data::Capability(caps))
            }),
        )),
        CRLF,
    ));

    let (remaining, (_, _, response, _)) = parser(input)?;

    Ok((remaining, response))
}

/// Status condition
///
/// resp-cond-state = ("OK" / "NO" / "BAD") SP resp-text
fn resp_cond_state(input: &[u8]) -> IResult<&[u8], (&str, Option<Code>, txt)> {
    let parser = tuple((
        alt((tag_no_case("OK"), tag_no_case("NO"), tag_no_case("BAD"))),
        SP,
        resp_text,
    ));

    let (remaining, (raw_status, _, (maybe_code, text))) = parser(input)?;

    Ok((
        remaining,
        (from_utf8(raw_status).expect("can't fail"), maybe_code, text), // FIXME(perf): use from_utf8_unchecked
    ))
}

/// response-done = response-tagged / response-fatal
fn response_done(input: &[u8]) -> IResult<&[u8], Status> {
    alt((response_tagged, response_fatal))(input)
}

/// response-tagged = tag SP resp-cond-state CRLF
fn response_tagged(input: &[u8]) -> IResult<&[u8], Status> {
    let parser = tuple((tag_imap, SP, resp_cond_state, CRLF));

    let (remaining, (tag, _, (raw_status, maybe_code, text), _)) = parser(input)?;

    let status = match raw_status.to_lowercase().as_ref() {
        "ok" => Status::Ok {
            tag: Some(tag),
            code: maybe_code,
            text: text.to_owned(),
        },
        "no" => Status::No {
            tag: Some(tag),
            code: maybe_code,
            text: text.to_owned(),
        },
        "bad" => Status::Bad {
            tag: Some(tag),
            code: maybe_code,
            text: text.to_owned(),
        },
        _ => unreachable!(),
    };

    Ok((remaining, status))
}

/// Server closes connection immediately
///
/// response-fatal = "*" SP resp-cond-bye CRLF
fn response_fatal(input: &[u8]) -> IResult<&[u8], Status> {
    let parser = tuple((tag(b"*"), SP, resp_cond_bye, CRLF));

    let (remaining, (_, _, (maybe_code, text), _)) = parser(input)?;

    Ok((remaining, {
        Status::Bye {
            code: maybe_code,
            text: text.to_owned(),
        }
    }))
}
