// Copyright (C) Parity Technologies (UK) Ltd.
// This file is part of Polkadot.

// Polkadot is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.

// Polkadot is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.

// You should have received a copy of the GNU General Public License
// along with Polkadot.  If not, see <http://www.gnu.org/licenses/>.

#![cfg(test)]

use crate::{
	mock::*,
	tests::{ALICE, BOB, FEE_AMOUNT, INITIAL_BALANCE, SEND_AMOUNT},
};
use frame_support::{
	assert_ok,
	traits::{tokens::fungibles::Inspect, Currency},
	weights::Weight,
};
use polkadot_parachain_primitives::primitives::Id as ParaId;
use sp_runtime::{traits::AccountIdConversion, DispatchError, ModuleError};
use xcm::prelude::*;
use xcm_executor::traits::ConvertLocation;

// Helper function to deduplicate testing different teleport types.
fn do_test_and_verify_teleport_assets<Call: FnOnce()>(
	expected_beneficiary: MultiLocation,
	call: Call,
	expected_weight_limit: WeightLimit,
) {
	let balances = vec![
		(ALICE, INITIAL_BALANCE),
		(ParaId::from(OTHER_PARA_ID).into_account_truncating(), INITIAL_BALANCE),
	];
	new_test_ext_with_balances(balances).execute_with(|| {
		let weight = BaseXcmWeight::get() * 3;
		assert_eq!(Balances::total_balance(&ALICE), INITIAL_BALANCE);
		// call extrinsic
		call();
		assert_eq!(Balances::total_balance(&ALICE), INITIAL_BALANCE - SEND_AMOUNT);
		assert_eq!(
			sent_xcm(),
			vec![(
				RelayLocation::get().into(),
				Xcm(vec![
					ReceiveTeleportedAsset((Here, SEND_AMOUNT).into()),
					ClearOrigin,
					buy_limited_execution((Here, SEND_AMOUNT), expected_weight_limit),
					DepositAsset {
						assets: AllCounted(1).into(),
						beneficiary: expected_beneficiary
					},
				]),
			)]
		);
		let versioned_sent = VersionedXcm::from(sent_xcm().into_iter().next().unwrap().1);
		let _check_v2_ok: xcm::v2::Xcm<()> = versioned_sent.try_into().unwrap();
		assert_eq!(
			last_event(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
	});
}

/// Test `teleport_assets`
///
/// Asserts that the sender's balance is decreased as a result of execution of
/// local effects.
#[test]
fn teleport_assets_works() {
	let beneficiary: MultiLocation = AccountId32 { network: None, id: BOB.into() }.into();
	do_test_and_verify_teleport_assets(
		beneficiary,
		|| {
			assert_ok!(XcmPallet::teleport_assets(
				RuntimeOrigin::signed(ALICE),
				Box::new(RelayLocation::get().into()),
				Box::new(beneficiary.into()),
				Box::new((Here, SEND_AMOUNT).into()),
				0,
			));
		},
		Unlimited,
	);
}

/// Test `limited_teleport_assets`
///
/// Asserts that the sender's balance is decreased as a result of execution of
/// local effects.
#[test]
fn limited_teleport_assets_works() {
	let beneficiary: MultiLocation = AccountId32 { network: None, id: BOB.into() }.into();
	let weight_limit = WeightLimit::Limited(Weight::from_parts(5000, 5000));
	let expected_weight_limit = weight_limit.clone();
	do_test_and_verify_teleport_assets(
		beneficiary,
		|| {
			assert_ok!(XcmPallet::limited_teleport_assets(
				RuntimeOrigin::signed(ALICE),
				Box::new(RelayLocation::get().into()),
				Box::new(beneficiary.into()),
				Box::new((Here, SEND_AMOUNT).into()),
				0,
				weight_limit,
			));
		},
		expected_weight_limit,
	);
}

/// `limited_teleport_assets` should fail for filtered assets
#[test]
fn limited_teleport_filtered_assets_disallowed() {
	let beneficiary: MultiLocation = AccountId32 { network: None, id: BOB.into() }.into();
	new_test_ext_with_balances(vec![(ALICE, INITIAL_BALANCE)]).execute_with(|| {
		let result = XcmPallet::limited_teleport_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(FilteredTeleportLocation::get().into()),
			Box::new(beneficiary.into()),
			Box::new(FilteredTeleportAsset::get().into()),
			0,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [2, 0, 0, 0],
				message: Some("Filtered")
			}))
		);
	});
}

/// Test `reserve_transfer_assets_with_paid_router_works`
///
/// Asserts that the sender's balance is decreased and the beneficiary's balance
/// is increased. Verifies the correct message is sent and event is emitted.
/// Verifies that XCM router fees (`SendXcm::validate` -> `MultiAssets`) are withdrawn from correct
/// user account and deposited to a correct target account (`XcmFeesTargetAccount`).
#[test]
fn reserve_transfer_assets_with_paid_router_works() {
	let user_account = AccountId::from(XCM_FEES_NOT_WAIVED_USER_ACCOUNT);
	let paid_para_id = Para3000::get();
	let balances = vec![
		(user_account.clone(), INITIAL_BALANCE),
		(ParaId::from(paid_para_id).into_account_truncating(), INITIAL_BALANCE),
		(XcmFeesTargetAccount::get(), INITIAL_BALANCE),
	];
	new_test_ext_with_balances(balances).execute_with(|| {
		let xcm_router_fee_amount = Para3000PaymentAmount::get();
		let weight = BaseXcmWeight::get();
		let dest: MultiLocation =
			Junction::AccountId32 { network: None, id: user_account.clone().into() }.into();
		assert_eq!(Balances::total_balance(&user_account), INITIAL_BALANCE);
		assert_ok!(XcmPallet::reserve_transfer_assets(
			RuntimeOrigin::signed(user_account.clone()),
			Box::new(Parachain(paid_para_id).into()),
			Box::new(dest.into()),
			Box::new((Here, SEND_AMOUNT).into()),
			0,
		));

		// XCM_FEES_NOT_WAIVED_USER_ACCOUNT spent amount
		assert_eq!(
			Balances::free_balance(user_account),
			INITIAL_BALANCE - SEND_AMOUNT - xcm_router_fee_amount
		);

		// Destination account (parachain account) has amount
		let para_acc: AccountId = ParaId::from(paid_para_id).into_account_truncating();
		assert_eq!(Balances::free_balance(para_acc), INITIAL_BALANCE + SEND_AMOUNT);

		// XcmFeesTargetAccount where should lend xcm_router_fee_amount
		assert_eq!(
			Balances::free_balance(XcmFeesTargetAccount::get()),
			INITIAL_BALANCE + xcm_router_fee_amount
		);

		let dest_para: MultiLocation = Parachain(paid_para_id).into();
		assert_eq!(
			sent_xcm(),
			vec![(
				dest_para,
				Xcm(vec![
					ReserveAssetDeposited((Parent, SEND_AMOUNT).into()),
					ClearOrigin,
					buy_execution((Parent, SEND_AMOUNT)),
					DepositAsset { assets: AllCounted(1).into(), beneficiary: dest },
				]),
			)]
		);
		let mut last_events = last_events(5).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
		// balances events
		last_events.next().unwrap();
		last_events.next().unwrap();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: dest,
				fees: Para3000PaymentMultiAssets::get(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));
	});
}

fn set_up_foreign_asset(
	reserve_para_id: u32,
	inner_junction: Option<Junction>,
	initial_amount: u128,
	is_sufficient: bool,
) -> (MultiLocation, AccountId, MultiLocation) {
	let reserve_location =
		RelayLocation::get().pushed_with_interior(Parachain(reserve_para_id)).unwrap();
	let reserve_sovereign_account =
		SovereignAccountOf::convert_location(&reserve_location).unwrap();

	let foreign_asset_id_multilocation = if let Some(junction) = inner_junction {
		reserve_location.pushed_with_interior(junction).unwrap()
	} else {
		reserve_location
	};

	// create sufficient (to be used as fees as well) foreign asset (0 total issuance)
	assert_ok!(Assets::force_create(
		RuntimeOrigin::root(),
		foreign_asset_id_multilocation,
		BOB,
		is_sufficient,
		1
	));
	// this asset should have been teleported/reserve-transferred in, but for this test we just
	// mint it locally.
	assert_ok!(Assets::mint(
		RuntimeOrigin::signed(BOB),
		foreign_asset_id_multilocation,
		ALICE,
		initial_amount
	));

	(reserve_location, reserve_sovereign_account, foreign_asset_id_multilocation)
}

// Helper function that provides correct `fee_index` after `sort()` done by
// `vec![MultiAsset, MultiAsset].into()`.
fn into_multiassets_checked(
	fee_asset: MultiAsset,
	transfer_asset: MultiAsset,
) -> (MultiAssets, usize, MultiAsset, MultiAsset) {
	let assets: MultiAssets = vec![fee_asset.clone(), transfer_asset.clone()].into();
	let fee_index = if assets.get(0).unwrap().eq(&fee_asset) { 0 } else { 1 };
	(assets, fee_index, fee_asset, transfer_asset)
}

/// Test `limited_reserve_transfer_assets` with local asset reserve and local fee reserve.
///
/// Transferring native asset (local reserve) to some `OTHER_PARA_ID` (no teleport trust).
/// Using native asset for fees as well.
///
/// ```nocompile
///    Here (source)                               OTHER_PARA_ID (destination)
///    |  `assets` reserve
///    |  `fees` reserve
///    |
///    |  1. execute `TransferReserveAsset(assets_and_fees_batched_together)`
///    |     \--> sends `ReserveAssetDeposited(both), ClearOrigin, BuyExecution(fees), DepositAsset`
///    \------------------------------------------>
/// ```
#[test]
fn limited_reserve_transfer_assets_with_local_asset_reserve_and_local_fee_reserve_works() {
	let balances = vec![
		(ALICE, INITIAL_BALANCE),
		(ParaId::from(OTHER_PARA_ID).into_account_truncating(), INITIAL_BALANCE),
	];

	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	let weight_limit = WeightLimit::Limited(Weight::from_parts(5000, 5000));
	let expected_weight_limit = weight_limit.clone();
	let expected_beneficiary = beneficiary;
	let dest: MultiLocation = Parachain(OTHER_PARA_ID).into();

	new_test_ext_with_balances(balances).execute_with(|| {
		let weight = BaseXcmWeight::get();
		assert_eq!(Balances::total_balance(&ALICE), INITIAL_BALANCE);
		// call extrinsic
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new((Here, SEND_AMOUNT).into()),
			0,
			weight_limit,
		));
		// Alice spent amount
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE - SEND_AMOUNT);
		// Destination account (parachain account) has amount
		let para_acc: AccountId = ParaId::from(OTHER_PARA_ID).into_account_truncating();
		assert_eq!(Balances::free_balance(para_acc), INITIAL_BALANCE + SEND_AMOUNT);
		assert_eq!(
			sent_xcm(),
			vec![(
				dest,
				Xcm(vec![
					ReserveAssetDeposited((Parent, SEND_AMOUNT).into()),
					ClearOrigin,
					buy_limited_execution((Parent, SEND_AMOUNT), expected_weight_limit),
					DepositAsset {
						assets: AllCounted(1).into(),
						beneficiary: expected_beneficiary
					},
				]),
			)]
		);
		let mut last_events = last_events(3).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: expected_beneficiary,
				fees: MultiAssets::new(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));
	});
}

/// Test `reserve_transfer_assets` with destination asset reserve and local fee reserve.
///
/// Transferring foreign asset (`FOREIGN_ASSET_RESERVE_PARA_ID` reserve) to
/// `FOREIGN_ASSET_RESERVE_PARA_ID` (no teleport trust).
/// Using native asset (local reserve) for fees.
///
/// ```nocompile
///    Here (source)                               FOREIGN_ASSET_RESERVE_PARA_ID (destination)
///    |  `fees` reserve                          `assets` reserve
///    |
///    |  1. execute `TransferReserveAsset(fees)`
///    |     \-> sends `ReserveAssetDeposited(fees), ClearOrigin, BuyExecution(fees), DepositAsset`
///    |  2. execute `InitiateReserveWithdraw(assets)`
///    |     \--> sends `WithdrawAsset(assets), ClearOrigin, BuyExecution(fees), DepositAsset`
///    \------------------------------------------>
/// ```
///
/// Asserts that the sender's balance is decreased and the beneficiary's balance
/// is increased. Verifies the correct message is sent and event is emitted.
#[test]
fn reserve_transfer_assets_with_destination_asset_reserve_and_local_fee_reserve_works() {
	let weight = BaseXcmWeight::get() * 3;
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create non-sufficient foreign asset BLA (0 total issuance)
		let foreign_initial_amount = 142;
		let (reserve_location, reserve_sovereign_account, foreign_asset_id_multilocation) =
			set_up_foreign_asset(
				FOREIGN_ASSET_RESERVE_PARA_ID,
				Some(FOREIGN_ASSET_INNER_JUNCTION),
				foreign_initial_amount,
				false,
			);

		// transfer destination is reserve location (no teleport trust)
		let dest = reserve_location;

		let (assets, fee_index, fee_asset, xfer_asset) = into_multiassets_checked(
			// native asset for fee - local reserve
			(MultiLocation::here(), FEE_AMOUNT).into(),
			// foreign asset to transfer - destination reserve
			(foreign_asset_id_multilocation, SEND_AMOUNT).into(),
		);

		// reanchor according to test-case
		let context = UniversalLocation::get();
		let expected_fee = fee_asset.reanchored(&dest, context).unwrap();
		let expected_asset = xfer_asset.reanchored(&dest, context).unwrap();

		// balances checks before
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		));

		let mut last_events = last_events(3).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);

		// Alice spent (transferred) amount
		assert_eq!(
			Assets::balance(foreign_asset_id_multilocation, ALICE),
			foreign_initial_amount - SEND_AMOUNT
		);
		// Alice used native asset for fees
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE - FEE_AMOUNT);
		// Destination account (parachain account) added native reserve used as fee to balances
		assert_eq!(Balances::free_balance(reserve_sovereign_account.clone()), FEE_AMOUNT);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, reserve_sovereign_account), 0);
		// Verify total and active issuance of foreign BLA have decreased (burned on
		// reserve-withdraw)
		let expected_issuance = foreign_initial_amount - SEND_AMOUNT;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_issuance);

		// Verify sent XCM program
		assert_eq!(
			sent_xcm(),
			vec![(
				dest,
				// `fees` are being sent through local-reserve transfer because fee reserve is
				// local chain; `assets` are burned on source and withdrawn from SA here
				Xcm(vec![
					ReserveAssetDeposited((Parent, FEE_AMOUNT).into()),
					buy_limited_execution(expected_fee, Unlimited),
					WithdrawAsset(expected_asset.into()),
					ClearOrigin,
					DepositAsset { assets: AllCounted(2).into(), beneficiary },
				])
			)]
		);
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: beneficiary,
				fees: MultiAssets::new(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));
	});
}

/// Test `reserve_transfer_assets` with remote asset reserve and local fee reserve.
///
/// Transferring foreign asset (reserve on `FOREIGN_ASSET_RESERVE_PARA_ID`) to `OTHER_PARA_ID`.
/// Using native (local reserve) as fee should be disallowed.
#[test]
fn reserve_transfer_assets_with_remote_asset_reserve_and_local_fee_reserve_disallowed() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create non-sufficient foreign asset BLA (0 total issuance)
		let foreign_initial_amount = 142;
		let (_, _, foreign_asset_id_multilocation) = set_up_foreign_asset(
			FOREIGN_ASSET_RESERVE_PARA_ID,
			Some(FOREIGN_ASSET_INNER_JUNCTION),
			foreign_initial_amount,
			false,
		);

		// transfer destination is OTHER_PARA_ID (foreign asset needs to go through its reserve
		// chain)
		let dest = RelayLocation::get().pushed_with_interior(Parachain(OTHER_PARA_ID)).unwrap();

		let (assets, fee_index, _, _) = into_multiassets_checked(
			// native asset for fee - local reserve
			(MultiLocation::here(), FEE_AMOUNT).into(),
			// foreign asset to transfer - remote reserve
			(foreign_asset_id_multilocation, SEND_AMOUNT).into(),
		);

		// balances checks before
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// try the transfer
		let result = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [22, 0, 0, 0],
				message: Some("InvalidAssetUnsupportedReserve")
			}))
		);

		// Alice transferred nothing
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		// Alice spent native asset for fees
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		// Verify total and active issuance of foreign BLA asset have decreased (burned on
		// reserve-withdraw)
		let expected_issuance = foreign_initial_amount;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_issuance);
	});
}

/// Test `reserve_transfer_assets` with local asset reserve and destination fee reserve.
///
/// Transferring native asset (local reserve) to `USDC_RESERVE_PARA_ID` (no teleport trust). Using
/// foreign asset (`USDC_RESERVE_PARA_ID` reserve) for fees.
///
/// ```nocompile
///    Here (source)                               USDC_RESERVE_PARA_ID (destination)
///    |  `assets` reserve                         `fees` reserve
///    |
///    |  1. execute `InitiateReserveWithdraw(fees)`
///    |     \--> sends `WithdrawAsset(fees), ClearOrigin, BuyExecution(fees), DepositAsset`
///    |  2. execute `TransferReserveAsset(assets)`
///    |     \-> sends `ReserveAssetDeposited(assets), ClearOrigin, BuyExecution(fees), DepositAsset`
///    \------------------------------------------>
/// ```
#[test]
fn reserve_transfer_assets_with_local_asset_reserve_and_destination_fee_reserve_works() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDC (0 total issuance)
		let usdc_initial_local_amount = 142;
		let (usdc_reserve_location, usdc_chain_sovereign_account, usdc_id_multilocation) =
			set_up_foreign_asset(
				USDC_RESERVE_PARA_ID,
				Some(USDC_INNER_JUNCTION),
				usdc_initial_local_amount,
				true,
			);

		// native assets transfer to fee reserve location (no teleport trust)
		let dest = usdc_reserve_location;

		let (assets, fee_index, fee_asset, xfer_asset) = into_multiassets_checked(
			// usdc for fees (is sufficient on local chain too) - destination reserve
			(usdc_id_multilocation, FEE_AMOUNT).into(),
			// native asset to transfer (not used for fees) - local reserve
			(MultiLocation::here(), SEND_AMOUNT).into(),
		);

		// reanchor according to test-case
		let context = UniversalLocation::get();
		let expected_fee = fee_asset.reanchored(&dest, context).unwrap();
		let expected_asset = xfer_asset.reanchored(&dest, context).unwrap();

		// balances checks before
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		));
		let weight = BaseXcmWeight::get() * 3;
		let mut last_events = last_events(3).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: beneficiary,
				fees: MultiAssets::new(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));

		// Alice spent (fees) amount
		assert_eq!(
			Assets::balance(usdc_id_multilocation, ALICE),
			usdc_initial_local_amount - FEE_AMOUNT
		);
		// Alice used native asset for transfer
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE - SEND_AMOUNT);
		// Sovereign account of dest parachain holds `SEND_AMOUNT` native asset in local reserve
		assert_eq!(Balances::free_balance(usdc_chain_sovereign_account.clone()), SEND_AMOUNT);
		assert_eq!(Assets::balance(usdc_id_multilocation, usdc_chain_sovereign_account), 0);
		// Verify total and active issuance of USDC have decreased (burned on reserve-withdraw)
		let expected_issuance = usdc_initial_local_amount - FEE_AMOUNT;
		assert_eq!(Assets::total_issuance(usdc_id_multilocation), expected_issuance);
		assert_eq!(Assets::active_issuance(usdc_id_multilocation), expected_issuance);

		// Verify sent XCM program
		assert_eq!(
			sent_xcm(),
			vec![(
				dest,
				Xcm(vec![
					// fees are being sent through destination-reserve transfer because fee reserve
					// is destination chain
					WithdrawAsset(expected_fee.clone().into()),
					buy_limited_execution(expected_fee, Unlimited),
					// transfer is through local-reserve transfer because `assets` (native asset)
					// have local reserve
					ReserveAssetDeposited(expected_asset.into()),
					ClearOrigin,
					DepositAsset { assets: AllCounted(2).into(), beneficiary },
				])
			)]
		);
	});
}

/// Test `reserve_transfer_assets` with destination asset reserve and destination fee reserve.
///
/// ```nocompile
///    Here (source)                               FOREIGN_ASSET_RESERVE_PARA_ID (destination)
///    |                                           `fees` reserve
///    |                                           `assets` reserve
///    |
///    |  1. execute `InitiateReserveWithdraw(assets_and_fees_batched_together)`
///    |     \--> sends `WithdrawAsset(batch), ClearOrigin, BuyExecution(fees), DepositAsset`
///    \------------------------------------------>
/// ```
#[test]
fn reserve_transfer_assets_with_destination_asset_reserve_and_destination_fee_reserve_works() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// we'll send just this foreign asset back to its reserve location and use it for fees as
		// well
		let foreign_initial_amount = 142;
		let (reserve_location, reserve_sovereign_account, foreign_asset_id_multilocation) =
			set_up_foreign_asset(
				FOREIGN_ASSET_RESERVE_PARA_ID,
				Some(FOREIGN_ASSET_INNER_JUNCTION),
				foreign_initial_amount,
				true,
			);

		// transfer destination is reserve location
		let dest = reserve_location;
		let assets: MultiAssets = vec![(foreign_asset_id_multilocation, SEND_AMOUNT).into()].into();
		let fee_index = 0;

		// reanchor according to test-case
		let mut expected_assets = assets.clone();
		expected_assets.reanchor(&dest, UniversalLocation::get()).unwrap();

		// balances checks before
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index,
			Unlimited,
		));

		let weight = BaseXcmWeight::get() * 2;
		let mut last_events = last_events(3).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: beneficiary,
				fees: MultiAssets::new(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));

		// Alice spent (transferred) amount
		assert_eq!(
			Assets::balance(foreign_asset_id_multilocation, ALICE),
			foreign_initial_amount - SEND_AMOUNT
		);
		// Alice's native asset balance is untouched
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		// Reserve sovereign account has same balances
		assert_eq!(Balances::free_balance(reserve_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, reserve_sovereign_account), 0);
		// Verify total and active issuance of foreign BLA have decreased (burned on
		// reserve-withdraw)
		let expected_issuance = foreign_initial_amount - SEND_AMOUNT;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_issuance);

		// Verify sent XCM program
		assert_eq!(
			sent_xcm(),
			vec![(
				Parachain(FOREIGN_ASSET_RESERVE_PARA_ID).into(),
				Xcm(vec![
					WithdrawAsset(expected_assets.clone()),
					ClearOrigin,
					buy_limited_execution(expected_assets.get(0).unwrap().clone(), Unlimited),
					DepositAsset { assets: AllCounted(1).into(), beneficiary },
				]),
			)]
		);
	});
}

/// Test `reserve_transfer_assets` with remote asset reserve and destination fee reserve is
/// disallowed.
///
/// Transferring foreign asset (reserve on `FOREIGN_ASSET_RESERVE_PARA_ID`) to
/// `USDC_RESERVE_PARA_ID`. Using USDC (destination reserve) as fee.
#[test]
fn reserve_transfer_assets_with_remote_asset_reserve_and_destination_fee_reserve_disallowed() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDC (0 total issuance)
		let usdc_initial_local_amount = 42;
		let (usdc_chain, _, usdc_id_multilocation) = set_up_foreign_asset(
			USDC_RESERVE_PARA_ID,
			Some(USDC_INNER_JUNCTION),
			usdc_initial_local_amount,
			true,
		);

		// create non-sufficient foreign asset BLA (0 total issuance)
		let foreign_initial_amount = 142;
		let (_, _, foreign_asset_id_multilocation) = set_up_foreign_asset(
			FOREIGN_ASSET_RESERVE_PARA_ID,
			Some(FOREIGN_ASSET_INNER_JUNCTION),
			foreign_initial_amount,
			false,
		);

		// transfer destination is USDC chain (foreign asset BLA needs to go through its separate
		// reserve chain)
		let dest = usdc_chain;

		let (assets, fee_index, _, _) = into_multiassets_checked(
			// USDC for fees (is sufficient on local chain too) - destination reserve
			(usdc_id_multilocation, FEE_AMOUNT).into(),
			// foreign asset to transfer (not used for fees) - remote reserve
			(foreign_asset_id_multilocation, SEND_AMOUNT).into(),
		);

		// balances checks before
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		let result = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [22, 0, 0, 0],
				message: Some("InvalidAssetUnsupportedReserve")
			}))
		);

		// Alice native asset untouched
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		let expected_usdc_issuance = usdc_initial_local_amount;
		assert_eq!(Assets::total_issuance(usdc_id_multilocation), expected_usdc_issuance);
		assert_eq!(Assets::active_issuance(usdc_id_multilocation), expected_usdc_issuance);
		let expected_bla_issuance = foreign_initial_amount;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
	});
}

/// Test `reserve_transfer_assets` with local asset reserve and remote fee reserve is disallowed.
///
/// Transferring native asset (local reserve) to `OTHER_PARA_ID` (no teleport trust). Using foreign
/// asset (`USDC_RESERVE_PARA_ID` remote reserve) for fees.
#[test]
fn reserve_transfer_assets_with_local_asset_reserve_and_remote_fee_reserve_disallowed() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDC (0 total issuance)
		let usdc_initial_local_amount = 142;
		let (_, usdc_chain_sovereign_account, usdc_id_multilocation) = set_up_foreign_asset(
			USDC_RESERVE_PARA_ID,
			Some(USDC_INNER_JUNCTION),
			usdc_initial_local_amount,
			true,
		);

		// transfer destination is some other parachain != fee reserve location (no teleport trust)
		let dest = RelayLocation::get().pushed_with_interior(Parachain(OTHER_PARA_ID)).unwrap();
		let dest_sovereign_account = SovereignAccountOf::convert_location(&dest).unwrap();

		let (assets, fee_index, _, _) = into_multiassets_checked(
			// USDC for fees (is sufficient on local chain too) - remote reserve
			(usdc_id_multilocation, FEE_AMOUNT).into(),
			// native asset to transfer (not used for fees) - local reserve
			(MultiLocation::here(), SEND_AMOUNT).into(),
		);

		// balances checks before
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		let result = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [22, 0, 0, 0],
				message: Some("InvalidAssetUnsupportedReserve")
			}))
		);
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		// Sovereign account of reserve parachain is unchanged
		assert_eq!(Balances::free_balance(usdc_chain_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(usdc_id_multilocation, usdc_chain_sovereign_account), 0);
		assert_eq!(Balances::free_balance(dest_sovereign_account), 0);
		let expected_usdc_issuance = usdc_initial_local_amount;
		assert_eq!(Assets::total_issuance(usdc_id_multilocation), expected_usdc_issuance);
		assert_eq!(Assets::active_issuance(usdc_id_multilocation), expected_usdc_issuance);
	});
}

/// Test `reserve_transfer_assets` with destination asset reserve and remote fee reserve is
/// disallowed.
///
/// Transferring native asset (local reserve) to `OTHER_PARA_ID` (no teleport trust). Using foreign
/// asset (`USDC_RESERVE_PARA_ID` remote reserve) for fees.
#[test]
fn reserve_transfer_assets_with_destination_asset_reserve_and_remote_fee_reserve_disallowed() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDC (0 total issuance)
		let usdc_initial_local_amount = 42;
		let (_, usdc_chain_sovereign_account, usdc_id_multilocation) = set_up_foreign_asset(
			USDC_RESERVE_PARA_ID,
			Some(USDC_INNER_JUNCTION),
			usdc_initial_local_amount,
			true,
		);

		// create non-sufficient foreign asset BLA (0 total issuance)
		let foreign_initial_amount = 142;
		let (reserve_location, foreign_sovereign_account, foreign_asset_id_multilocation) =
			set_up_foreign_asset(
				FOREIGN_ASSET_RESERVE_PARA_ID,
				Some(FOREIGN_ASSET_INNER_JUNCTION),
				foreign_initial_amount,
				false,
			);

		// transfer destination is asset reserve location
		let dest = reserve_location;
		let dest_sovereign_account = foreign_sovereign_account;

		let (assets, fee_index, _, _) = into_multiassets_checked(
			// USDC for fees (is sufficient on local chain too) - remote reserve
			(usdc_id_multilocation, FEE_AMOUNT).into(),
			// foreign asset to transfer (not used for fees) - destination reserve
			(foreign_asset_id_multilocation, SEND_AMOUNT).into(),
		);

		// balances checks before
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		let result = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [22, 0, 0, 0],
				message: Some("InvalidAssetUnsupportedReserve")
			}))
		);
		// Alice native asset untouched
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		assert_eq!(Balances::free_balance(usdc_chain_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(usdc_id_multilocation, usdc_chain_sovereign_account), 0);
		assert_eq!(Balances::free_balance(dest_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, dest_sovereign_account), 0);
		let expected_usdc_issuance = usdc_initial_local_amount;
		assert_eq!(Assets::total_issuance(usdc_id_multilocation), expected_usdc_issuance);
		assert_eq!(Assets::active_issuance(usdc_id_multilocation), expected_usdc_issuance);
		let expected_bla_issuance = foreign_initial_amount;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
	});
}

/// Test `reserve_transfer_assets` with remote asset reserve and (same) remote fee reserve.
///
/// Transferring native asset (local reserve) to `OTHER_PARA_ID` (no teleport trust). Using foreign
/// asset (`USDC_RESERVE_PARA_ID` remote reserve) for fees.
///
/// ```nocompile
///    | chain `A`           |  chain `C`                      |  chain `B`
///    | Here (source)       |  USDC_RESERVE_PARA_ID           |  OTHER_PARA_ID (destination)
///    |                     |  `fees` reserve                 |
///    |                     |  `assets` reserve               |
///    |
///    |  1. `A` executes `InitiateReserveWithdraw(both)` dest `C`
///    |     -----------------> `C` executes `DepositReserveAsset(both)` dest `B`
///    |                             --------------------------> `DepositAsset(both)`
/// ```
#[test]
fn reserve_transfer_assets_with_remote_asset_reserve_and_remote_fee_reserve_works() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDC (0 total issuance)
		let usdc_initial_local_amount = 142;
		let (usdc_chain, usdc_chain_sovereign_account, usdc_id_multilocation) =
			set_up_foreign_asset(
				USDC_RESERVE_PARA_ID,
				Some(USDC_INNER_JUNCTION),
				usdc_initial_local_amount,
				true,
			);

		// transfer destination is some other parachain
		let dest = RelayLocation::get().pushed_with_interior(Parachain(OTHER_PARA_ID)).unwrap();

		let assets: MultiAssets = vec![(usdc_id_multilocation, SEND_AMOUNT).into()].into();
		let fee_index = 0;

		// reanchor according to test-case
		let context = UniversalLocation::get();
		let expected_dest_on_reserve = dest.reanchored(&usdc_chain, context).unwrap();
		let fees = assets.get(fee_index).unwrap().clone();
		let (fees_half_1, fees_half_2) = XcmPallet::halve_fees(fees).unwrap();
		let mut expected_assets_on_reserve = assets.clone();
		expected_assets_on_reserve.reanchor(&usdc_chain, context).unwrap();
		let expected_fee_on_reserve = fees_half_1.reanchored(&usdc_chain, context).unwrap();
		let expected_fee_on_dest = fees_half_2.reanchored(&dest, context).unwrap();

		// balances checks before
		assert_eq!(Assets::balance(usdc_id_multilocation, ALICE), usdc_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		));
		assert!(matches!(
			last_event(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(_) })
		));

		// Alice spent (transferred) amount
		assert_eq!(
			Assets::balance(usdc_id_multilocation, ALICE),
			usdc_initial_local_amount - SEND_AMOUNT
		);
		// Alice's native asset balance is untouched
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		// Destination account (parachain account) has expected (same) balances
		assert_eq!(Balances::free_balance(usdc_chain_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(usdc_id_multilocation, usdc_chain_sovereign_account), 0);
		// Verify total and active issuance of USDC have decreased (burned on reserve-withdraw)
		let expected_usdc_issuance = usdc_initial_local_amount - SEND_AMOUNT;
		assert_eq!(Assets::total_issuance(usdc_id_multilocation), expected_usdc_issuance);
		assert_eq!(Assets::active_issuance(usdc_id_multilocation), expected_usdc_issuance);

		// Verify sent XCM program
		assert_eq!(
			sent_xcm(),
			vec![(
				// first message sent to reserve chain
				usdc_chain,
				Xcm(vec![
					WithdrawAsset(expected_assets_on_reserve),
					ClearOrigin,
					BuyExecution { fees: expected_fee_on_reserve, weight_limit: Unlimited },
					DepositReserveAsset {
						assets: Wild(AllCounted(1)),
						// final destination is `dest` as seen by `reserve`
						dest: expected_dest_on_reserve,
						// message sent onward to `dest`
						xcm: Xcm(vec![
							buy_limited_execution(expected_fee_on_dest, Unlimited),
							DepositAsset { assets: AllCounted(1).into(), beneficiary }
						])
					}
				])
			)],
		);
	});
}

/// Test `reserve_transfer_assets` with local asset reserve and teleported fee.
///
/// Transferring native asset (local reserve) to `USDT_PARA_ID`. Using teleport-trusted USDT for
/// fees.
///
/// ```nocompile
///    Here (source)                               USDT_PARA_ID (destination)
///    |  `assets` reserve                         `fees` teleport-trust
///    |
///    |  1. execute `InitiateTeleport(fees)`
///    |     \--> sends `ReceiveTeleportedAsset(fees), .., DepositAsset(fees)`
///    |  2. execute `TransferReserveAsset(assets)`
///    |     \-> sends `ReserveAssetDeposited(assets), ClearOrigin, BuyExecution(fees), DepositAsset`
///    \------------------------------------------>
/// ```
#[test]
fn reserve_transfer_assets_with_local_asset_reserve_and_teleported_fee_works() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDT (0 total issuance)
		let usdt_initial_local_amount = 42;
		let (usdt_chain, usdt_chain_sovereign_account, usdt_id_multilocation) =
			set_up_foreign_asset(USDT_PARA_ID, None, usdt_initial_local_amount, true);

		// native assets transfer destination is USDT chain (teleport trust only for USDT)
		let dest = usdt_chain;

		let (assets, fee_index, fee_asset, xfer_asset) = into_multiassets_checked(
			// USDT for fees (is sufficient on local chain too) - teleported
			(usdt_id_multilocation, FEE_AMOUNT).into(),
			// native asset to transfer (not used for fees) - local reserve
			(MultiLocation::here(), SEND_AMOUNT).into(),
		);

		// reanchor according to test-case
		let context = UniversalLocation::get();
		let expected_fee = fee_asset.reanchored(&dest, context).unwrap();
		let expected_asset = xfer_asset.reanchored(&dest, context).unwrap();

		// balances checks before
		assert_eq!(Assets::balance(usdt_id_multilocation, ALICE), usdt_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		));
		let weight = BaseXcmWeight::get() * 3;
		let mut last_events = last_events(3).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: beneficiary,
				fees: MultiAssets::new(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));
		// Alice spent (fees) amount
		assert_eq!(
			Assets::balance(usdt_id_multilocation, ALICE),
			usdt_initial_local_amount - FEE_AMOUNT
		);
		// Alice used native asset for transfer
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE - SEND_AMOUNT);
		// Sovereign account of dest parachain holds `SEND_AMOUNT` native asset in local reserve
		assert_eq!(Balances::free_balance(usdt_chain_sovereign_account.clone()), SEND_AMOUNT);
		assert_eq!(Assets::balance(usdt_id_multilocation, usdt_chain_sovereign_account), 0);
		// Verify total and active issuance have decreased (teleported)
		let expected_usdt_issuance = usdt_initial_local_amount - FEE_AMOUNT;
		assert_eq!(Assets::total_issuance(usdt_id_multilocation), expected_usdt_issuance);
		assert_eq!(Assets::active_issuance(usdt_id_multilocation), expected_usdt_issuance);

		// Verify sent XCM program
		assert_eq!(
			sent_xcm(),
			vec![(
				dest,
				Xcm(vec![
					// fees are teleported to destination chain
					ReceiveTeleportedAsset(expected_fee.clone().into()),
					buy_limited_execution(expected_fee, Unlimited),
					// transfer is through local-reserve transfer because `assets` (native
					// asset) have local reserve
					ReserveAssetDeposited(expected_asset.into()),
					ClearOrigin,
					DepositAsset { assets: AllCounted(2).into(), beneficiary },
				])
			)]
		);
	});
}

/// Test `reserve_transfer_assets` with destination asset reserve and teleported fee.
///
/// Transferring foreign asset (destination reserve) to `FOREIGN_ASSET_RESERVE_PARA_ID`. Using
/// teleport-trusted USDT for fees.
///
/// ```nocompile
///    Here (source)                               FOREIGN_ASSET_RESERVE_PARA_ID (destination)
///    |                                           `fees` (USDT) teleport-trust
///    |                                           `assets` reserve
///    |
///    |  1. execute `InitiateTeleport(fees)`
///    |     \--> sends `ReceiveTeleportedAsset(fees), .., DepositAsset(fees)`
///    |  2. execute `InitiateReserveWithdraw(assets)`
///    |     \--> sends `WithdrawAsset(asset), ClearOrigin, BuyExecution(fees), DepositAsset`
///    \------------------------------------------>
/// ```
#[test]
fn reserve_transfer_assets_with_destination_asset_reserve_and_teleported_fee_works() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDT (0 total issuance)
		let usdt_initial_local_amount = 42;
		let (_, usdt_chain_sovereign_account, usdt_id_multilocation) =
			set_up_foreign_asset(USDT_PARA_ID, None, usdt_initial_local_amount, true);

		// create non-sufficient foreign asset BLA (0 total issuance)
		let foreign_initial_amount = 142;
		let (reserve_location, foreign_sovereign_account, foreign_asset_id_multilocation) =
			set_up_foreign_asset(
				FOREIGN_ASSET_RESERVE_PARA_ID,
				Some(FOREIGN_ASSET_INNER_JUNCTION),
				foreign_initial_amount,
				false,
			);

		// transfer destination is asset reserve location
		let dest = reserve_location;
		let dest_sovereign_account = foreign_sovereign_account;

		let (assets, fee_index, fee_asset, xfer_asset) = into_multiassets_checked(
			// USDT for fees (is sufficient on local chain too) - teleported
			(usdt_id_multilocation, FEE_AMOUNT).into(),
			// foreign asset to transfer (not used for fees) - destination reserve
			(foreign_asset_id_multilocation, SEND_AMOUNT).into(),
		);

		// reanchor according to test-case
		let context = UniversalLocation::get();
		let expected_fee = fee_asset.reanchored(&dest, context).unwrap();
		let expected_asset = xfer_asset.reanchored(&dest, context).unwrap();

		// balances checks before
		assert_eq!(Assets::balance(usdt_id_multilocation, ALICE), usdt_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		assert_ok!(XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		));
		let weight = BaseXcmWeight::get() * 4;
		let mut last_events = last_events(3).into_iter();
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Attempted { outcome: Outcome::Complete(weight) })
		);
		assert_eq!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::FeesPaid {
				paying: beneficiary,
				fees: MultiAssets::new(),
			})
		);
		assert!(matches!(
			last_events.next().unwrap(),
			RuntimeEvent::XcmPallet(crate::Event::Sent { .. })
		));
		// Alice native asset untouched
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		// Alice spent USDT for fees
		assert_eq!(
			Assets::balance(usdt_id_multilocation, ALICE),
			usdt_initial_local_amount - FEE_AMOUNT
		);
		// Alice transferred BLA
		assert_eq!(
			Assets::balance(foreign_asset_id_multilocation, ALICE),
			foreign_initial_amount - SEND_AMOUNT
		);
		// Verify balances of USDT reserve parachain
		assert_eq!(Balances::free_balance(usdt_chain_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(usdt_id_multilocation, usdt_chain_sovereign_account), 0);
		// Verify balances of transferred-asset reserve parachain
		assert_eq!(Balances::free_balance(dest_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, dest_sovereign_account), 0);
		// Verify total and active issuance of USDT have decreased (teleported)
		let expected_usdt_issuance = usdt_initial_local_amount - FEE_AMOUNT;
		assert_eq!(Assets::total_issuance(usdt_id_multilocation), expected_usdt_issuance);
		assert_eq!(Assets::active_issuance(usdt_id_multilocation), expected_usdt_issuance);
		// Verify total and active issuance of foreign BLA asset have decreased (burned on
		// reserve-withdraw)
		let expected_bla_issuance = foreign_initial_amount - SEND_AMOUNT;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_bla_issuance);

		// Verify sent XCM program
		assert_eq!(
			sent_xcm(),
			vec![(
				dest,
				Xcm(vec![
					// fees are teleported to destination chain
					ReceiveTeleportedAsset(expected_fee.clone().into()),
					buy_limited_execution(expected_fee, Unlimited),
					// assets are withdrawn from origin's local SA
					WithdrawAsset(expected_asset.into()),
					ClearOrigin,
					DepositAsset { assets: AllCounted(2).into(), beneficiary },
				])
			)]
		);
	});
}

/// Test `reserve_transfer_assets` with remote asset reserve and teleported fee is disallowed.
///
/// Transferring foreign asset (reserve on `FOREIGN_ASSET_RESERVE_PARA_ID`) to `USDT_PARA_ID`.
/// Using teleport-trusted USDT for fees.
#[test]
fn reserve_transfer_assets_with_remote_asset_reserve_and_teleported_fee_disallowed() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();
	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDT (0 total issuance)
		let usdt_initial_local_amount = 42;
		let (usdt_chain, usdt_chain_sovereign_account, usdt_id_multilocation) =
			set_up_foreign_asset(USDT_PARA_ID, None, usdt_initial_local_amount, true);

		// create non-sufficient foreign asset BLA (0 total issuance)
		let foreign_initial_amount = 142;
		let (_, reserve_sovereign_account, foreign_asset_id_multilocation) = set_up_foreign_asset(
			FOREIGN_ASSET_RESERVE_PARA_ID,
			Some(FOREIGN_ASSET_INNER_JUNCTION),
			foreign_initial_amount,
			false,
		);

		// transfer destination is USDT chain (foreign asset needs to go through its reserve chain)
		let dest = usdt_chain;

		let (assets, fee_index, _, _) = into_multiassets_checked(
			// USDT for fees (is sufficient on local chain too) - teleported
			(usdt_id_multilocation, FEE_AMOUNT).into(),
			// foreign asset to transfer (not used for fees) - remote reserve
			(foreign_asset_id_multilocation, SEND_AMOUNT).into(),
		);

		// balances checks before
		assert_eq!(Assets::balance(usdt_id_multilocation, ALICE), usdt_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		let result = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [22, 0, 0, 0],
				message: Some("InvalidAssetUnsupportedReserve")
			}))
		);
		// Alice native asset untouched
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		assert_eq!(Assets::balance(usdt_id_multilocation, ALICE), usdt_initial_local_amount);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, ALICE), foreign_initial_amount);
		assert_eq!(Balances::free_balance(usdt_chain_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(usdt_id_multilocation, usdt_chain_sovereign_account), 0);
		assert_eq!(Balances::free_balance(reserve_sovereign_account.clone()), 0);
		assert_eq!(Assets::balance(foreign_asset_id_multilocation, reserve_sovereign_account), 0);
		let expected_usdt_issuance = usdt_initial_local_amount;
		assert_eq!(Assets::total_issuance(usdt_id_multilocation), expected_usdt_issuance);
		assert_eq!(Assets::active_issuance(usdt_id_multilocation), expected_usdt_issuance);
		let expected_bla_issuance = foreign_initial_amount;
		assert_eq!(Assets::total_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
		assert_eq!(Assets::active_issuance(foreign_asset_id_multilocation), expected_bla_issuance);
	});
}

/// Test `reserve_transfer_assets` single asset which is teleportable - should fail.
///
/// Attempting to reserve-transfer teleport-trusted USDT to `USDT_PARA_ID` should fail.
#[test]
fn reserve_transfer_assets_with_teleportable_asset_fails() {
	let balances = vec![(ALICE, INITIAL_BALANCE)];
	let beneficiary: MultiLocation =
		Junction::AccountId32 { network: None, id: ALICE.into() }.into();

	new_test_ext_with_balances(balances).execute_with(|| {
		// create sufficient foreign asset USDT (0 total issuance)
		let usdt_initial_local_amount = 42;
		let (usdt_chain, usdt_chain_sovereign_account, usdt_id_multilocation) =
			set_up_foreign_asset(USDT_PARA_ID, None, usdt_initial_local_amount, true);

		// transfer destination is USDT chain (foreign asset needs to go through its reserve chain)
		let dest = usdt_chain;
		let assets: MultiAssets = vec![(usdt_id_multilocation, FEE_AMOUNT).into()].into();
		let fee_index = 0;

		// balances checks before
		assert_eq!(Assets::balance(usdt_id_multilocation, ALICE), usdt_initial_local_amount);
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);

		// do the transfer
		let res = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(dest.into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			res,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [2, 0, 0, 0],
				message: Some("Filtered")
			}))
		);
		// Alice native asset is still same
		assert_eq!(Balances::free_balance(ALICE), INITIAL_BALANCE);
		// Alice USDT balance is still same
		assert_eq!(Assets::balance(usdt_id_multilocation, ALICE), usdt_initial_local_amount);
		// No USDT moved to sovereign account of reserve parachain
		assert_eq!(Assets::balance(usdt_id_multilocation, usdt_chain_sovereign_account), 0);
		// Verify total and active issuance of USDT are still the same
		assert_eq!(Assets::total_issuance(usdt_id_multilocation), usdt_initial_local_amount);
		assert_eq!(Assets::active_issuance(usdt_id_multilocation), usdt_initial_local_amount);
	});
}

/// Test `reserve_transfer_assets` with teleportable fee that is filtered - should fail.
#[test]
fn reserve_transfer_assets_with_filtered_teleported_fee_disallowed() {
	let beneficiary: MultiLocation = AccountId32 { network: None, id: BOB.into() }.into();
	new_test_ext_with_balances(vec![(ALICE, INITIAL_BALANCE)]).execute_with(|| {
		let (assets, fee_index, _, _) = into_multiassets_checked(
			// FilteredTeleportAsset for fees - teleportable but filtered
			FilteredTeleportAsset::get().into(),
			// native asset to transfer (not used for fees) - local reserve
			(MultiLocation::here(), SEND_AMOUNT).into(),
		);
		let result = XcmPallet::limited_reserve_transfer_assets(
			RuntimeOrigin::signed(ALICE),
			Box::new(FilteredTeleportLocation::get().into()),
			Box::new(beneficiary.into()),
			Box::new(assets.into()),
			fee_index as u32,
			Unlimited,
		);
		assert_eq!(
			result,
			Err(DispatchError::Module(ModuleError {
				index: 4,
				error: [2, 0, 0, 0],
				message: Some("Filtered")
			}))
		);
	});
}
