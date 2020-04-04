use crate::{
    parse::core::nz_number,
    types::{SeqNo, Sequence},
};
use nom::{
    branch::alt,
    bytes::streaming::{tag, tag_no_case},
    combinator::map,
    combinator::value,
    multi::separated_nonempty_list,
    sequence::tuple,
    IResult,
};

/// ; errata id: 261
/// sequence-set = (seq-number / seq-range) ["," sequence-set]
///                  ; set of seq-number values, regardless of order.
///                  ; Servers MAY coalesce overlaps and/or execute the
///                  ; sequence in any order.
///                  ; Example: a message sequence number set of
///                  ; 2,4:7,9,12:* for a mailbox with 15 messages is
///                  ; equivalent to 2,4,5,6,7,9,12,13,14,15
///                  ; Example: a message sequence number set of *:4,5:7
///                  ; for a mailbox with 10 messages is equivalent to
///                  ; 10,9,8,7,6,5,4,5,6,7 and MAY be reordered and
///                  ; overlap coalesced to be 4,5,6,7,8,9,10.
pub fn sequence_set(input: &[u8]) -> IResult<&[u8], Vec<Sequence>> {
    let parser = separated_nonempty_list(
        // TODO: I made a mistake here by not using nonempty. Check other occurences.
        tag(b","),
        alt((
            // TODO: ordering is important
            map(seq_range, |(from, to)| Sequence::Range(from, to)),
            map(seq_number, Sequence::Single),
        )),
    );

    let (remaining, parsed_sequence_set) = parser(input)?;

    Ok((remaining, parsed_sequence_set))
}

/// seq-range = seq-number ":" seq-number
///               ; two seq-number values and all values between
///               ; these two regardless of order.
///               ; Example: 2:4 and 4:2 are equivalent and indicate
///               ; values 2, 3, and 4.
///               ; Example: a unique identifier sequence range of
///               ; 3291:* includes the UID of the last message in
///               ; the mailbox, even if that value is less than 3291.
pub fn seq_range(input: &[u8]) -> IResult<&[u8], (SeqNo, SeqNo)> {
    let parser = tuple((seq_number, tag_no_case(b":"), seq_number));

    let (remaining, (from, _, to)) = parser(input)?;

    Ok((remaining, (from, to)))
}

/// seq-number = nz-number / "*"
///                ; message sequence number (COPY, FETCH, STORE
///                ; commands) or unique identifier (UID COPY,
///                ; UID FETCH, UID STORE commands).
///                ; * represents the largest number in use.  In
///                ; the case of message sequence numbers, it is
///                ; the number of messages in a non-empty mailbox.
///                ; In the case of unique identifiers, it is the
///                ; unique identifier of the last message in the
///                ; mailbox or, if the mailbox is empty, the
///                ; mailbox's current UIDNEXT value.
///                ; The server should respond with a tagged BAD
///                ; response to a command that uses a message
///                ; sequence number greater than the number of
///                ; messages in the selected mailbox.  This
///                ; includes "*" if the selected mailbox is empty.
pub fn seq_number(input: &[u8]) -> IResult<&[u8], SeqNo> {
    let parser = alt((
        map(nz_number, SeqNo::Value),
        value(SeqNo::Unlimited, tag(b"*")),
    ));

    let (remaining, parsed_seq_number) = parser(input)?;

    Ok((remaining, parsed_seq_number))
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_sequence_set() {
        let (rem, val) = sequence_set(b"1:*?").unwrap();
        println!("{:?}, {:?}", rem, val);

        let (rem, val) = sequence_set(b"1:*,5?").unwrap();
        println!("{:?}, {:?}", rem, val);
    }

    #[test]
    fn test_seq_number() {
        // Must not be 0.
        assert!(seq_number(b"0?").is_err());

        let (rem, val) = seq_number(b"1?").unwrap();
        println!("{:?}, {:?}", rem, val);

        let (rem, val) = seq_number(b"*?").unwrap();
        println!("{:?}, {:?}", rem, val);
    }

    #[test]
    fn test_seq_range() {
        // Must not be 0.
        assert!(seq_range(b"0:1?").is_err());

        assert_eq!(
            (SeqNo::Value(1), SeqNo::Value(2)),
            seq_range(b"1:2?").unwrap().1
        );
        assert_eq!(
            (SeqNo::Value(1), SeqNo::Unlimited),
            seq_range(b"1:*?").unwrap().1
        );
        assert_eq!(
            (SeqNo::Unlimited, SeqNo::Value(10)),
            seq_range(b"*:10?").unwrap().1
        );
    }
}