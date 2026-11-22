//! Issue #1180: Cross-shard communication, first slice.
//!
//! Pure per-channel sequence tracking giving in-order, exactly-once delivery
//! semantics and message counters. No storage, no contract invocation and no
//! retry/transport is implemented here; callers own persistence.

/// State for one ordered channel between two shards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ChannelState {
    /// Next sequence number the sender will assign.
    pub next_send: u64,
    /// Next sequence number the receiver expects.
    pub next_expected: u64,
    /// Count of messages delivered in order.
    pub delivered: u64,
    /// Count of duplicate messages rejected.
    pub duplicates: u64,
}

/// Outcome of offering an incoming message to the receiver.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Delivery {
    /// Message was the expected one and is accepted.
    Accepted,
    /// Message was already delivered; ignore it.
    Duplicate,
    /// Message skipped ahead; `missing` is the expected sequence number.
    Gap { missing: u64 },
}

/// Errors from channel helpers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessagingError {
    /// A sequence counter would overflow.
    Overflow,
}

/// Assigns and returns the next outgoing sequence number.
pub fn assign_sequence(state: &mut ChannelState) -> Result<u64, MessagingError> {
    let seq = state.next_send;
    state.next_send = seq.checked_add(1).ok_or(MessagingError::Overflow)?;
    Ok(seq)
}

/// Offers an incoming message with sequence `seq` to the receiver.
///
/// Only the exactly-expected sequence advances the channel, which preserves
/// ordering; gaps are reported so the caller can request redelivery rather
/// than silently losing a message.
pub fn receive(state: &mut ChannelState, seq: u64) -> Result<Delivery, MessagingError> {
    if seq < state.next_expected {
        state.duplicates = state.duplicates.saturating_add(1);
        return Ok(Delivery::Duplicate);
    }
    if seq > state.next_expected {
        return Ok(Delivery::Gap { missing: state.next_expected });
    }
    state.next_expected = seq.checked_add(1).ok_or(MessagingError::Overflow)?;
    state.delivered = state.delivered.saturating_add(1);
    Ok(Delivery::Accepted)
}

/// Number of sent messages not yet acknowledged as delivered in order.
pub fn in_flight(state: &ChannelState) -> u64 {
    state.next_send.saturating_sub(state.next_expected)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_order_delivery() {
        let mut s = ChannelState::default();
        let a = assign_sequence(&mut s).unwrap();
        let b = assign_sequence(&mut s).unwrap();
        assert_eq!((a, b), (0, 1));
        assert_eq!(in_flight(&s), 2);
        assert_eq!(receive(&mut s, 0), Ok(Delivery::Accepted));
        assert_eq!(receive(&mut s, 1), Ok(Delivery::Accepted));
        assert_eq!(s.delivered, 2);
        assert_eq!(in_flight(&s), 0);
    }

    #[test]
    fn gap_and_duplicate_detected() {
        let mut s = ChannelState::default();
        assert_eq!(receive(&mut s, 2), Ok(Delivery::Gap { missing: 0 }));
        assert_eq!(receive(&mut s, 0), Ok(Delivery::Accepted));
        assert_eq!(receive(&mut s, 0), Ok(Delivery::Duplicate));
        assert_eq!(s.duplicates, 1);
        assert_eq!(s.next_expected, 1);
    }

    #[test]
    fn overflow_is_an_error() {
        let mut s = ChannelState { next_send: u64::MAX, ..Default::default() };
        assert_eq!(assign_sequence(&mut s), Err(MessagingError::Overflow));
    }
}
