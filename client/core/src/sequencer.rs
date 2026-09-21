//! Packet ordering and loss concealment.
//!
//! The decision is made on the *distance* between consecutive sequence
//! numbers, not on the count of missing packets, and the two differ by one:
//!
//! | distance        | missing packets | action                       |
//! |-----------------|-----------------|------------------------------|
//! | 0               | —               | duplicate: drop              |
//! | 1               | 0               | in order: play               |
//! | 2..=6           | 1..=5           | fill the gap with silence    |
//! | 7..=0x8000_0000 | 6 or more       | too far behind: resync       |
//! | > 0x8000_0000   | —               | from the past: drop          |
//!
//! Distance is computed with wrapping subtraction, so the scheme survives the
//! 32-bit sequence counter rolling over.

/// Largest gap, measured as sequence distance, that is concealed with silence
/// rather than triggering a resync. Distance 6 means 5 packets were lost.
pub const MAX_CONCEALED_DISTANCE: u32 = 6;

/// Distances above this are treated as packets from the past (late or
/// reordered) rather than as a huge forward jump.
const PAST_THRESHOLD: u32 = 0x8000_0000;

/// What the receiver should do with an incoming packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Play the packet.
    Play,
    /// Insert this many packets' worth of silence, then play the packet.
    ConcealThenPlay { silent_packets: u32 },
    /// Discard buffered audio, then play the packet as a fresh start.
    ResyncThenPlay,
    /// Drop the packet: a duplicate, or one that arrived after its successors.
    Drop,
}

/// Tracks the last accepted sequence number.
#[derive(Debug, Default, Clone)]
pub struct Sequencer {
    last: Option<u32>,
}

impl Sequencer {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide what to do with a packet carrying `sequence`, and advance state
    /// if the packet is accepted.
    pub fn accept(&mut self, sequence: u32) -> Action {
        let Some(last) = self.last else {
            self.last = Some(sequence);
            return Action::Play;
        };

        let distance = sequence.wrapping_sub(last);
        let action = match distance {
            0 => Action::Drop,
            d if d > PAST_THRESHOLD => Action::Drop,
            1 => Action::Play,
            d if d <= MAX_CONCEALED_DISTANCE => Action::ConcealThenPlay { silent_packets: d - 1 },
            _ => Action::ResyncThenPlay,
        };

        if action != Action::Drop {
            self.last = Some(sequence);
        }
        action
    }

    /// Forget the stream, e.g. when the device disconnects. The next packet is
    /// accepted unconditionally.
    pub fn reset(&mut self) {
        self.last = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn after(first: u32, next: u32) -> Action {
        let mut s = Sequencer::new();
        assert_eq!(s.accept(first), Action::Play);
        s.accept(next)
    }

    #[test]
    fn first_packet_always_plays() {
        assert_eq!(Sequencer::new().accept(123_456), Action::Play);
    }

    #[test]
    fn in_order_plays() {
        assert_eq!(after(10, 11), Action::Play);
    }

    #[test]
    fn duplicate_is_dropped() {
        assert_eq!(after(10, 10), Action::Drop);
    }

    #[test]
    fn distance_two_conceals_exactly_one_packet() {
        // Easy to get off by one: distance 2 == exactly 1 packet lost.
        assert_eq!(after(10, 12), Action::ConcealThenPlay { silent_packets: 1 });
    }

    #[test]
    fn distance_six_is_the_last_concealed_gap() {
        assert_eq!(after(10, 16), Action::ConcealThenPlay { silent_packets: 5 });
    }

    #[test]
    fn distance_seven_resyncs() {
        // 6 packets lost is one past the concealment window.
        assert_eq!(after(10, 17), Action::ResyncThenPlay);
    }

    #[test]
    fn late_packet_is_dropped_not_treated_as_a_huge_jump() {
        assert_eq!(after(100, 99), Action::Drop);
    }

    #[test]
    fn survives_sequence_wraparound() {
        assert_eq!(after(u32::MAX, 0), Action::Play);
        assert_eq!(after(u32::MAX - 1, 1), Action::ConcealThenPlay { silent_packets: 2 });
    }

    #[test]
    fn dropped_packets_do_not_advance_state() {
        let mut s = Sequencer::new();
        s.accept(10);
        assert_eq!(s.accept(9), Action::Drop); // late
        assert_eq!(s.accept(11), Action::Play, "state must still be at 10");
    }

    #[test]
    fn reset_accepts_anything_next() {
        let mut s = Sequencer::new();
        s.accept(10);
        s.reset();
        assert_eq!(s.accept(3), Action::Play);
    }
}
