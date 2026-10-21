#![cfg(test)]
//! Coverage for `approve` authorisation and `transfer_batch` events, pause
//! behaviour and resource cost (issues #528-#531).
use super::*;
use compliance::{ComplianceContract, ComplianceContractClient};
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, String, Vec,
};

struct Setup {
    env: Env,
    token: AssetTokenContractClient<'static>,
    compliance: ComplianceContractClient<'static>,
    admin: Address,
}

fn setup(supply: i128) -> Setup {
    let env = Env::default();
    env.mock_all_auths();
    let compliance_id = env.register(ComplianceContract, ());
    let compliance = ComplianceContractClient::new(&env, &compliance_id);
    let admin = Address::generate(&env);
    compliance.initialize(&admin);
    allow(&env, &compliance, &admin, &admin);
    let token_id = env.register(AssetTokenContract, ());
    let token = AssetTokenContractClient::new(&env, &token_id);
    token.initialize(
        &admin,
        &String::from_str(&env, "Manhattan Loft"),
        &String::from_str(&env, "MLOFT"),
        &String::from_str(&env, "real_estate"),
        &supply,
        &2u32,
        &compliance_id,
        &String::from_str(&env, "A tokenized NYC loft"),
        &50_000_000i128,
    );
    Setup {
        env,
        token,
        compliance,
        admin,
    }
}

fn allow(env: &Env, c: &ComplianceContractClient, admin: &Address, who: &Address) {
    c.add_to_allowlist(admin, who, &String::from_str(env, "US"), &0);
}

// ---- issue #531: approve's unauthorised caller ----

/// `approve` is authorised by `from` alone. A call that carries no
/// authorisation for `from` is rejected and no allowance is written.
#[test]
fn test_approve_without_owner_auth_is_rejected() {
    let s = setup(1_000);
    let spender = Address::generate(&s.env);
    // Drop the blanket auth mock: nobody has authorised anything now.
    s.env.mock_auths(&[]);
    let exp = s.env.ledger().sequence() + 100;
    let res = s.token.try_approve(&s.admin, &spender, &100, &exp);
    assert!(res.is_err(), "approve must require auth from `from`");
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

/// `approve` is intentionally open to any address that authorises itself: it
/// needs no admin role and no compliance allowlisting, because it only moves
/// the caller's own allowance. Documented current behaviour.
#[test]
fn test_approve_open_to_unprivileged_self_authorised_caller() {
    let s = setup(1_000);
    let stranger = Address::generate(&s.env);
    let spender = Address::generate(&s.env);
    assert!(!s.compliance.is_allowed(&stranger));
    let exp = s.env.ledger().sequence() + 100;
    s.token.approve(&stranger, &spender, &50, &exp);
    assert_eq!(s.token.allowance(&stranger, &spender), 50);
    // It did not touch the admin's allowance for the same spender.
    assert_eq!(s.token.allowance(&s.admin, &spender), 0);
}

// ---- issue #530: transfer_batch while paused / deactivated ----

/// The token has no separate "deactivated" flag: `pause` is the only
/// halt state. While paused, `transfer_batch` reverts deliberately with
/// `Error::Paused` (#6), before any balance is touched.
#[test]
fn test_transfer_batch_rejected_when_paused_with_deliberate_error() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    allow(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);

    let mut batch = Vec::new(&s.env);
    batch.push_back((bob.clone(), 100i128));
    let res = s.token.try_transfer_batch(&s.admin, &batch);
    assert_eq!(res, Err(Ok(Error::Paused)));
    assert_eq!(s.token.balance(&s.admin), 1_000);
    assert_eq!(s.token.balance(&bob), 0);
}

/// Pause is checked even for an empty batch, and unpausing restores service.
#[test]
fn test_transfer_batch_paused_empty_batch_and_unpause_restores() {
    let s = setup(1_000);
    let bob = Address::generate(&s.env);
    allow(&s.env, &s.compliance, &s.admin, &bob);
    s.token.pause(&s.admin);

    let empty: Vec<(Address, i128)> = Vec::new(&s.env);
    assert_eq!(
        s.token.try_transfer_batch(&s.admin, &empty),
        Err(Ok(Error::Paused))
    );

    s.token.unpause(&s.admin);
    let mut batch = Vec::new(&s.env);
    batch.push_back((bob.clone(), 100i128));
    s.token.transfer_batch(&s.admin, &batch);
    assert_eq!(s.token.balance(&bob), 100);
}
