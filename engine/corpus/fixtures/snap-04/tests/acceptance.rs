#[test]
fn adds_positive_values() {
    assert_eq!(failing::calc::add(2, 3), 5);
}

#[test]
fn adds_negative_values() {
    assert_eq!(failing::calc::add(-2, -3), -5);
}

#[test]
fn adds_mixed_values() {
    assert_eq!(failing::calc::add(5, -2), 3);
    assert_eq!(failing::calc::add(0, 0), 0);
}
