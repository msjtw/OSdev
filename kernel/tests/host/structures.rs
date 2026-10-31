#[path = "../../src/structures.rs"]
mod structures;

use structures::MonotonicCounter;

#[test]
fn counters_start_at_one_and_are_independent() {
    let mut first = MonotonicCounter::<3>::new();
    let mut second = MonotonicCounter::<3>::default();
    assert_eq!(first.next(), Some(1));
    assert_eq!(first.next(), Some(2));
    assert_eq!(second.next(), Some(1));
}

#[test]
fn exhausted_counter_does_not_wrap_or_restart() {
    let mut counter = MonotonicCounter::<2>::new();
    assert_eq!(counter.next(), Some(1));
    assert_eq!(counter.next(), Some(2));
    assert_eq!(counter.next(), None);
    assert_eq!(counter.next(), None);
    assert_eq!(MonotonicCounter::<0>::new().next(), None);
}
