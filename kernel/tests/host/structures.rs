mod structures {
    include!("../../src/structures.rs");

    // Host-only fixture; no kernel visibility change or runtime test hook.
    pub fn near_exhaustion() -> Counter {
        Counter { last: usize::MAX - 1 }
    }
}

use structures::Counter;

#[test]
fn counters_start_at_one_and_are_independent() {
    let mut first = Counter::new();
    let mut second = Counter::default();
    assert_eq!(first.next(), Some(1));
    assert_eq!(first.next(), Some(2));
    assert_eq!(second.next(), Some(1));
}

#[test]
fn exhausted_counter_does_not_wrap_or_restart() {
    let mut counter = structures::near_exhaustion();
    assert_eq!(counter.next(), Some(usize::MAX));
    assert_eq!(counter.next(), None);
    assert_eq!(counter.next(), None);
}
