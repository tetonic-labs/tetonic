#[test]
fn renamed_account_is_returned_by_the_public_api() {
    let account: refactor::models::Account = refactor::api::get_user();
    assert_eq!(account.id, 1);
}
