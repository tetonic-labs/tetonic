#[test]
fn sorts_unordered_users() {
    assert_eq!(misleading::sort_users(vec![3, 1, 2]), vec![1, 2, 3]);
}

#[test]
fn preserves_duplicates_and_extremes() {
    assert_eq!(
        misleading::sort_users(vec![u64::MAX, 0, 2, 2]),
        vec![0, 2, 2, u64::MAX]
    );
}

#[test]
fn handles_empty_and_sorted_users() {
    assert!(misleading::sort_users(vec![]).is_empty());
    assert_eq!(misleading::sort_users(vec![1, 2]), vec![1, 2]);
}
