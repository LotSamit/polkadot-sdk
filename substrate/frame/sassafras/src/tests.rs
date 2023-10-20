// This file is part of Substrate.

// Copyright (C) Parity Technologies (UK) Ltd.
// SPDX-License-Identifier: Apache-2.0

// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
// 	http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Tests for Sassafras pallet.

use crate::*;
use mock::*;

use sp_consensus_sassafras::Slot;
use sp_runtime::traits::Get;

fn h2b<const N: usize>(hex: &str) -> [u8; N] {
	array_bytes::hex2array_unchecked(hex)
}

fn b2h<const N: usize>(bytes: [u8; N]) -> String {
	array_bytes::bytes2hex("", &bytes)
}

#[test]
fn genesis_values_assumptions_check() {
	new_test_ext(4).execute_with(|| {
		assert_eq!(Sassafras::authorities().len(), 4);
		assert_eq!(EpochConfig::<Test>::get(), TEST_EPOCH_CONFIGURATION);
	});
}

// Tests if the sorted tickets are assigned to each slot outside-in.
#[test]
fn slot_ticket_id_outside_in_fetch() {
	let genesis_slot = Slot::from(100);
	let tickets_count = 6;

	// Current epoch tickets
	let curr_tickets: Vec<TicketId> = (0..tickets_count).map(|i| i as TicketId).collect();

	// Next epoch tickets
	let next_tickets: Vec<TicketId> =
		(0..tickets_count - 1).map(|i| (i + tickets_count) as TicketId).collect();

	new_test_ext(4).execute_with(|| {
		curr_tickets
			.iter()
			.enumerate()
			.for_each(|(i, id)| TicketsIds::<Test>::insert((0, i as u32), id));

		next_tickets
			.iter()
			.enumerate()
			.for_each(|(i, id)| TicketsIds::<Test>::insert((1, i as u32), id));

		TicketsMeta::<Test>::set(TicketsMetadata {
			tickets_count: [curr_tickets.len() as u32, next_tickets.len() as u32],
			unsorted_tickets_count: 0,
		});

		// Before initializing `GenesisSlot` value the pallet always return the first slot.
		// This is a kind of special hardcoded case that should never happen in practice
		// as the first thing the pallet does is to initialize the genesis slot.

		assert_eq!(Sassafras::slot_ticket_id(0.into()), Some(curr_tickets[1]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 0), Some(curr_tickets[1]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 1), Some(curr_tickets[1]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 100), Some(curr_tickets[1]));

		// Initialize genesis slot..
		GenesisSlot::<Test>::set(genesis_slot);

		// Try to fetch a ticket for a slot before current epoch.
		assert_eq!(Sassafras::slot_ticket_id(0.into()), None);

		// Current epoch tickets.
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 0), Some(curr_tickets[1]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 1), Some(curr_tickets[3]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 2), Some(curr_tickets[5]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 3), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 4), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 5), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 6), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 7), Some(curr_tickets[4]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 8), Some(curr_tickets[2]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 9), Some(curr_tickets[0]));

		// Next epoch tickets (note that only 5 tickets are available)
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 10), Some(next_tickets[1]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 11), Some(next_tickets[3]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 12), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 13), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 14), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 15), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 16), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 17), Some(next_tickets[4]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 18), Some(next_tickets[2]));
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 19), Some(next_tickets[0]));

		// Try to fetch tickets for slots beyend next epoch.
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 20), None);
		assert_eq!(Sassafras::slot_ticket_id(genesis_slot + 42), None);
	});
}

#[test]
fn on_first_block_after_genesis() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);

	ext.execute_with(|| {
		let start_slot = Slot::from(100);
		let start_block = 1;

		let digest = initialize_block(start_block, start_slot, Default::default(), &pairs[0]);

		// Post-initialization status

		assert!(SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot);
		assert_eq!(Sassafras::epoch_index(), 0);
		assert_eq!(Sassafras::current_epoch_start(), start_slot);
		assert_eq!(Sassafras::current_slot_index(), 0);
		assert_eq!(Sassafras::randomness(), [0; 32]);
		assert_eq!(NextRandomness::<Test>::get(), [0; 32]);
		assert_eq!(RandomnessAccumulator::<Test>::get(), [0; 32]);

		let header = finalize_block(start_block);

		// Post-finalization status

		assert!(!SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot);
		assert_eq!(Sassafras::epoch_index(), 0);
		assert_eq!(Sassafras::current_epoch_start(), start_slot);
		assert_eq!(Sassafras::current_slot_index(), 0);
		assert_eq!(Sassafras::randomness(), [0; 32]);
		assert_eq!(NextRandomness::<Test>::get(), [0; 32]);
		println!("{}", b2h(RandomnessAccumulator::<Test>::get()));
		assert_eq!(
			RandomnessAccumulator::<Test>::get(),
			h2b("416f7e78a0390e14677782ea22102ba749eb9de7d02df46b39d1e3d6e6759c62"),
		);

		// Header data check

		assert_eq!(header.digest.logs.len(), 2);
		assert_eq!(header.digest.logs[0], digest.logs[0]);

		// Genesis epoch start deposits consensus
		let consensus_log = sp_consensus_sassafras::digests::ConsensusLog::NextEpochData(
			sp_consensus_sassafras::digests::NextEpochDescriptor {
				authorities: NextAuthorities::<Test>::get().to_vec(),
				randomness: NextRandomness::<Test>::get(),
				config: None,
			},
		);
		let consensus_digest = DigestItem::Consensus(SASSAFRAS_ENGINE_ID, consensus_log.encode());
		assert_eq!(header.digest.logs[1], consensus_digest)
	})
}

#[test]
fn on_normal_block() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);
	let start_slot = Slot::from(100);
	let start_block = 1;
	let end_block = start_block + 1;

	ext.execute_with(|| {
		initialize_block(start_block, start_slot, Default::default(), &pairs[0]);

		// We don't want to trigger an epoch change in this test.
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();
		assert!(epoch_duration > end_block);

		// Progress to block 2
		let digest = progress_to_block(end_block, &pairs[0]).unwrap();

		// Post-initialization status

		assert!(SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot + 1);
		assert_eq!(Sassafras::epoch_index(), 0);
		assert_eq!(Sassafras::current_epoch_start(), start_slot);
		assert_eq!(Sassafras::current_slot_index(), 1);
		assert_eq!(Sassafras::randomness(), [0; 32]);
		assert_eq!(NextRandomness::<Test>::get(), [0; 32]);
		println!("{}", b2h(RandomnessAccumulator::<Test>::get()));
		assert_eq!(
			RandomnessAccumulator::<Test>::get(),
			h2b("416f7e78a0390e14677782ea22102ba749eb9de7d02df46b39d1e3d6e6759c62"),
		);

		let header = finalize_block(end_block);

		// Post-finalization status

		assert!(!SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot + 1);
		assert_eq!(Sassafras::epoch_index(), 0);
		assert_eq!(Sassafras::current_epoch_start(), start_slot);
		assert_eq!(Sassafras::current_slot_index(), 1);
		assert_eq!(Sassafras::randomness(), [0; 32]);
		assert_eq!(NextRandomness::<Test>::get(), [0; 32]);
		println!("{}", b2h(RandomnessAccumulator::<Test>::get()));
		assert_eq!(
			RandomnessAccumulator::<Test>::get(),
			h2b("eab1c5692bf3255ae46b2e732d061700fcd51ab57f029ad39983ceae5214a713"),
		);

		// Header data check

		assert_eq!(header.digest.logs.len(), 1);
		assert_eq!(header.digest.logs[0], digest.logs[0]);
	});
}

#[test]
fn produce_epoch_change_digest_no_config() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);

	ext.execute_with(|| {
		let start_slot = Slot::from(100);
		let start_block = 1;

		initialize_block(start_block, start_slot, Default::default(), &pairs[0]);

		// We want to trigger an epoch change in this test.
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();
		let end_block = start_block + epoch_duration;

		let digest = progress_to_block(end_block, &pairs[0]).unwrap();

		// Post-initialization status

		assert!(SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot + epoch_duration);
		assert_eq!(Sassafras::epoch_index(), 1);
		assert_eq!(Sassafras::current_epoch_start(), start_slot + epoch_duration);
		assert_eq!(Sassafras::current_slot_index(), 0);
		assert_eq!(Sassafras::randomness(), [0; 32]);
		println!("{}", b2h(NextRandomness::<Test>::get()));
		assert_eq!(
			NextRandomness::<Test>::get(),
			h2b("cb52dcf3b0caca956453d42004ac1b8005a26be669c2aaf534548e0b4c872a52"),
		);
		println!("{}", b2h(RandomnessAccumulator::<Test>::get()));
		assert_eq!(
			RandomnessAccumulator::<Test>::get(),
			h2b("ce3e3aeae02c85a8e0c8ee0ff0b120484df4551491ac2296e40147634ca4c58c"),
		);

		let header = finalize_block(end_block);

		// Post-finalization status

		assert!(!SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot + epoch_duration);
		assert_eq!(Sassafras::epoch_index(), 1);
		assert_eq!(Sassafras::current_epoch_start(), start_slot + epoch_duration);
		assert_eq!(Sassafras::current_slot_index(), 0);
		assert_eq!(Sassafras::randomness(), [0; 32]);
		println!("{}", b2h(NextRandomness::<Test>::get()));
		assert_eq!(
			NextRandomness::<Test>::get(),
			h2b("cb52dcf3b0caca956453d42004ac1b8005a26be669c2aaf534548e0b4c872a52"),
		);
		println!("{}", b2h(RandomnessAccumulator::<Test>::get()));
		assert_eq!(
			RandomnessAccumulator::<Test>::get(),
			h2b("1288d911ca5deb9c514149d4fdb64ebf94e63989e09e03bc69218319456d4ec9"),
		);

		// Header data check

		assert_eq!(header.digest.logs.len(), 2);
		assert_eq!(header.digest.logs[0], digest.logs[0]);
		// Deposits consensus log on epoch change
		let consensus_log = sp_consensus_sassafras::digests::ConsensusLog::NextEpochData(
			sp_consensus_sassafras::digests::NextEpochDescriptor {
				authorities: NextAuthorities::<Test>::get().to_vec(),
				randomness: NextRandomness::<Test>::get(),
				config: None,
			},
		);
		let consensus_digest = DigestItem::Consensus(SASSAFRAS_ENGINE_ID, consensus_log.encode());
		assert_eq!(header.digest.logs[1], consensus_digest)
	})
}

#[test]
fn produce_epoch_change_digest_with_config() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);

	ext.execute_with(|| {
		let start_slot = Slot::from(100);
		let start_block = 1;

		initialize_block(start_block, start_slot, Default::default(), &pairs[0]);

		let config = EpochConfiguration { redundancy_factor: 1, attempts_number: 123 };
		Sassafras::plan_config_change(RuntimeOrigin::root(), config).unwrap();

		// We want to trigger an epoch change in this test.
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();
		let end_block = start_block + epoch_duration;

		let digest = progress_to_block(end_block, &pairs[0]).unwrap();

		let header = finalize_block(end_block);

		// Header data check.
		// Skip pallet status checks that were already performed by other tests.

		assert_eq!(header.digest.logs.len(), 2);
		assert_eq!(header.digest.logs[0], digest.logs[0]);
		// Deposits consensus log on epoch change
		let consensus_log = sp_consensus_sassafras::digests::ConsensusLog::NextEpochData(
			sp_consensus_sassafras::digests::NextEpochDescriptor {
				authorities: NextAuthorities::<Test>::get().to_vec(),
				randomness: NextRandomness::<Test>::get(),
				config: Some(config), // We are mostly interested in this
			},
		);
		let consensus_digest = DigestItem::Consensus(SASSAFRAS_ENGINE_ID, consensus_log.encode());
		assert_eq!(header.digest.logs[1], consensus_digest)
	})
}

#[test]
fn segments_incremental_sort_works() {
	let (pairs, mut ext) = new_test_ext_with_pairs(1, false);
	let pair = &pairs[0];
	let segments_count = 14;
	let start_slot = Slot::from(100);
	let start_block = 1;

	ext.execute_with(|| {
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();
		// -3 just to have the last segment not full...
		let submitted_tickets_count = segments_count * SEGMENT_MAX_SIZE - 3;

		initialize_block(start_block, start_slot, Default::default(), pair);

		// Manually populate the segments to skip the threshold check
		let mut tickets = make_ticket_bodies(submitted_tickets_count, None);
		persist_next_epoch_tickets_as_segments(&tickets);

		// Proceed to half of the epoch (sortition should not have been started yet)
		let half_epoch_block = start_block + epoch_duration / 2;
		progress_to_block(half_epoch_block, pair);

		let mut unsorted_tickets_count = submitted_tickets_count;

		// Check that next epoch tickets sortition is not started yet
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, unsorted_tickets_count);
		assert_eq!(meta.tickets_count, [0, 0]);

		// Follow the incremental sortition block by block

		progress_to_block(half_epoch_block + 1, pair);
		unsorted_tickets_count -= 3 * SEGMENT_MAX_SIZE - 3;
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, unsorted_tickets_count,);
		assert_eq!(meta.tickets_count, [0, 0]);

		progress_to_block(half_epoch_block + 2, pair);
		unsorted_tickets_count -= 3 * SEGMENT_MAX_SIZE;
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, unsorted_tickets_count);
		assert_eq!(meta.tickets_count, [0, 0]);

		progress_to_block(half_epoch_block + 3, pair);
		unsorted_tickets_count -= 3 * SEGMENT_MAX_SIZE;
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, unsorted_tickets_count);
		assert_eq!(meta.tickets_count, [0, 0]);

		progress_to_block(half_epoch_block + 4, pair);
		unsorted_tickets_count -= 3 * SEGMENT_MAX_SIZE;
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, unsorted_tickets_count);
		assert_eq!(meta.tickets_count, [0, 0]);

		let header = finalize_block(half_epoch_block + 4);

		// Sort should be finished now.
		// Check that next epoch tickets count have the correct value.
		// Bigger ticket ids were discarded during sortition.
		unsorted_tickets_count -= 2 * SEGMENT_MAX_SIZE;
		assert_eq!(unsorted_tickets_count, 0);
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, unsorted_tickets_count);
		assert_eq!(meta.tickets_count, [0, epoch_duration as u32]);
		// Epoch change log should have been pushed as well
		assert_eq!(header.digest.logs.len(), 1);
		// No tickets for the current epoch
		assert_eq!(TicketsIds::<Test>::get((0, 0)), None);

		// Check persistence of "winning" tickets
		tickets.sort_by_key(|t| t.0);
		(0..epoch_duration as usize).into_iter().for_each(|i| {
			let id = TicketsIds::<Test>::get((1, i as u32)).unwrap();
			let body = TicketsData::<Test>::get(id).unwrap();
			assert_eq!((id, body), tickets[i]);
		});
		// Check removal of "loosing" tickets
		(epoch_duration as usize..tickets.len()).into_iter().for_each(|i| {
			assert!(TicketsIds::<Test>::get((1, i as u32)).is_none());
			assert!(TicketsData::<Test>::get(tickets[i].0).is_none());
		});

		// The next block will be the first produced on the new epoch.
		// At this point the tickets are found already sorted and ready to be used.
		let slot = Sassafras::current_slot() + 1;
		let number = System::block_number() + 1;
		initialize_block(number, slot, header.hash(), pair);
		let header = finalize_block(number);
		// Epoch changes digest is also produced
		assert_eq!(header.digest.logs.len(), 2);
	});
}

#[test]
fn tickets_fetch_works_after_epoch_change() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);
	let pair = &pairs[0];
	let start_slot = Slot::from(100);
	let start_block = 1;
	let submitted_tickets = 300;

	ext.execute_with(|| {
		initialize_block(start_block, start_slot, Default::default(), pair);

		// We don't want to trigger an epoch change in this test.
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();
		assert!(epoch_duration > 2);
		progress_to_block(2, &pairs[0]).unwrap();

		// Persist tickets as three different segments.
		let tickets = make_ticket_bodies(submitted_tickets, None);
		persist_next_epoch_tickets_as_segments(&tickets);

		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, submitted_tickets);
		assert_eq!(meta.tickets_count, [0, 0]);

		// Progress up to the last epoch slot (do not enact epoch change)
		progress_to_block(epoch_duration, &pairs[0]).unwrap();

		// At this point next epoch tickets should have been sorted and ready to be used
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, 0);
		assert_eq!(meta.tickets_count, [0, epoch_duration as u32]);

		// Compute and sort the tickets ids (aka tickets scores)
		let mut expected_ids: Vec<_> = tickets.into_iter().map(|(id, _)| id).collect();
		expected_ids.sort();
		expected_ids.truncate(epoch_duration as usize);

		// Check if we can fetch next epoch tickets ids (outside-in).
		let slot = Sassafras::current_slot();
		assert_eq!(Sassafras::slot_ticket_id(slot + 1).unwrap(), expected_ids[1]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 2).unwrap(), expected_ids[3]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 3).unwrap(), expected_ids[5]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 4).unwrap(), expected_ids[7]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 7).unwrap(), expected_ids[6]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 8).unwrap(), expected_ids[4]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 9).unwrap(), expected_ids[2]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 10).unwrap(), expected_ids[0]);
		assert!(Sassafras::slot_ticket_id(slot + 11).is_none());

		// Enact epoch change by progressing one more block

		progress_to_block(epoch_duration + 1, &pairs[0]).unwrap();

		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, 0);
		assert_eq!(meta.tickets_count, [0, 10]);

		// Check if we can fetch current epoch tickets ids (outside-in).
		let slot = Sassafras::current_slot();
		assert_eq!(Sassafras::slot_ticket_id(slot).unwrap(), expected_ids[1]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 1).unwrap(), expected_ids[3]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 2).unwrap(), expected_ids[5]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 3).unwrap(), expected_ids[7]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 6).unwrap(), expected_ids[6]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 7).unwrap(), expected_ids[4]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 8).unwrap(), expected_ids[2]);
		assert_eq!(Sassafras::slot_ticket_id(slot + 9).unwrap(), expected_ids[0]);
		assert!(Sassafras::slot_ticket_id(slot + 10).is_none());

		// Enact another epoch change, for which we don't have any ticket
		progress_to_block(2 * epoch_duration + 1, &pairs[0]).unwrap();
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta.unsorted_tickets_count, 0);
		assert_eq!(meta.tickets_count, [0, 0]);
	});
}

#[test]
fn block_allowed_to_skip_epochs() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);
	let pair = &pairs[0];
	let start_slot = Slot::from(100);
	let start_block = 1;

	ext.execute_with(|| {
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();

		initialize_block(start_block, start_slot, Default::default(), pair);

		let tickets = make_ticket_bodies(3, Some(pair));
		persist_next_epoch_tickets(&tickets);

		let next_random = NextRandomness::<Test>::get();

		// We want to skip 2 epochs in this test.
		let offset = 3 * epoch_duration;
		go_to_block(start_block + offset, start_slot + offset, &pairs[0]);

		// Post-initialization status

		assert!(SlotRandomness::<Test>::exists());
		assert_eq!(Sassafras::genesis_slot(), start_slot);
		assert_eq!(Sassafras::current_slot(), start_slot + offset);
		assert_eq!(Sassafras::epoch_index(), 3);
		assert_eq!(Sassafras::current_epoch_start(), start_slot + offset);
		assert_eq!(Sassafras::current_slot_index(), 0);

		// Tickets data has been discarded
		let meta = TicketsMeta::<Test>::get();
		assert_eq!(meta, TicketsMetadata::default());

		tickets.iter().for_each(|(id, _)| {
			let data = TicketsData::<Test>::get(id);
			assert!(data.is_none());
		});
		// We used the last known next epoch randomness as a fallback
		assert_eq!(next_random, Sassafras::randomness());
	});
}

#[test]
fn obsolete_tickets_are_removed_on_epoch_change() {
	let (pairs, mut ext) = new_test_ext_with_pairs(4, false);
	let pair = &pairs[0];
	let start_slot = Slot::from(100);
	let start_block = 1;

	ext.execute_with(|| {
		let epoch_duration: u64 = <Test as Config>::EpochDuration::get();

		initialize_block(start_block, start_slot, Default::default(), pair);

		let tickets = make_ticket_bodies(10, Some(pair));
		let mut epoch1_tickets = tickets[..4].to_vec();
		let mut epoch2_tickets = tickets[4..].to_vec();

		// Persist some tickets for next epoch (N)
		persist_next_epoch_tickets(&epoch1_tickets);
		assert_eq!(TicketsMeta::<Test>::get().tickets_count, [0, 4]);
		// Check next epoch tickets presence
		epoch1_tickets.sort_by_key(|t| t.0);
		(0..epoch1_tickets.len()).into_iter().for_each(|i| {
			let id = TicketsIds::<Test>::get((1, i as u32)).unwrap();
			let body = TicketsData::<Test>::get(id).unwrap();
			assert_eq!((id, body), epoch1_tickets[i]);
		});

		// Advance one epoch to enact the tickets
		go_to_block(start_block + epoch_duration, start_slot + epoch_duration, pair);
		assert_eq!(TicketsMeta::<Test>::get().tickets_count, [0, 4]);

		// Persist some tickets for next epoch (N+1)
		persist_next_epoch_tickets(&epoch2_tickets);
		assert_eq!(TicketsMeta::<Test>::get().tickets_count, [6, 4]);
		epoch2_tickets.sort_by_key(|t| t.0);
		// Check for this epoch and next epoch tickets presence
		(0..epoch1_tickets.len()).into_iter().for_each(|i| {
			let id = TicketsIds::<Test>::get((1, i as u32)).unwrap();
			let body = TicketsData::<Test>::get(id).unwrap();
			assert_eq!((id, body), epoch1_tickets[i]);
		});
		(0..epoch2_tickets.len()).into_iter().for_each(|i| {
			let id = TicketsIds::<Test>::get((0, i as u32)).unwrap();
			let body = TicketsData::<Test>::get(id).unwrap();
			assert_eq!((id, body), epoch2_tickets[i]);
		});

		// Advance to epoch 2 and check for cleanup

		go_to_block(start_block + 2 * epoch_duration, start_slot + 2 * epoch_duration, pair);
		assert_eq!(TicketsMeta::<Test>::get().tickets_count, [6, 0]);

		(0..epoch1_tickets.len()).into_iter().for_each(|i| {
			let id = TicketsIds::<Test>::get((1, i as u32)).unwrap();
			assert!(TicketsData::<Test>::get(id).is_none());
		});
		(0..epoch2_tickets.len()).into_iter().for_each(|i| {
			let id = TicketsIds::<Test>::get((0, i as u32)).unwrap();
			let body = TicketsData::<Test>::get(id).unwrap();
			assert_eq!((id, body), epoch2_tickets[i]);
		});
	})
}

const TICKETS_FILE: &str = "tickets.bin";

#[derive(Encode, Decode)]
struct PreBuiltTickets {
	authorities: Vec<AuthorityId>,
	tickets: Vec<TicketEnvelope>,
}

fn tickets_data_read() -> PreBuiltTickets {
	use std::{fs::File, io::Read};
	let mut file = File::open(TICKETS_FILE).unwrap();
	let mut buf = Vec::new();
	file.read_to_end(&mut buf).unwrap();
	PreBuiltTickets::decode(&mut &buf[..]).unwrap()
}

fn tickets_data_write(data: PreBuiltTickets) {
	use std::{fs::File, io::Write};
	let mut file = File::create(TICKETS_FILE).unwrap();
	let buf = data.encode();
	file.write_all(&buf).unwrap();
}

// For this test we use a set pre-constructed tickets from a file.
// Creating "too many" tickets on the fly is too much expensive.
//
// A valid ring-context is required for this test since we are passing though the
// `submit_ticket` call which tests for ticket validity.
#[test]
fn submit_tickets_with_ring_proof_check_works() {
	// env_logger::init();

	use sp_core::Pair as _;

	let (pairs, mut ext) = new_test_ext_with_pairs(20, true);
	let pair = &pairs[0];
	let submit_calls_count = 3;

	let PreBuiltTickets { authorities, tickets } = tickets_data_read();

	// Check if deserialized data has been generated for the correct set of authorities...
	assert!(authorities.iter().zip(pairs.iter()).all(|(auth, pair)| auth == &pair.public()));

	ext.execute_with(|| {
		let start_slot = Slot::from(100);
		let start_block = 1;

		// Tweak the epoch config to discard more or less the 50% of the tickets.
		// Data is known and constant.
		let mut config = EpochConfig::<Test>::get();
		config.redundancy_factor = 1;
		config.attempts_number = 1;
		EpochConfig::<Test>::set(config);

		initialize_block(start_block, start_slot, Default::default(), pair);

		// Check state before tickets submission
		assert_eq!(
			TicketsMeta::<Test>::get(),
			TicketsMetadata { unsorted_tickets_count: 0, tickets_count: [0, 0] },
		);

		// Submit the tickets
		let chunk_len = tickets.len().div_ceil(submit_calls_count); // as usize;
		tickets.chunks(chunk_len).for_each(|chunk| {
			let chunk = BoundedVec::truncate_from(chunk.to_vec());
			Sassafras::submit_tickets(RuntimeOrigin::none(), chunk).unwrap();
		});

		// Check state after submission
		assert_eq!(
			TicketsMeta::<Test>::get(),
			TicketsMetadata { unsorted_tickets_count: 11, tickets_count: [0, 0] },
		);

		finalize_block(start_block);

		// Check against the expected results given the known inputs
		assert_eq!(NextTicketsSegments::<Test>::get(0).len(), 11);
		assert_eq!(NextTicketsSegments::<Test>::get(1).len(), 0);
	})
}

#[test]
#[ignore]
fn serialize_test_tickets() {
	use super::*;
	use sp_core::crypto::Pair;

	let authorities_count = 20;
	let tickets_count = 20;

	let (pairs, mut ext) = new_test_ext_with_pairs(authorities_count, true);
	let pair = &pairs[0];

	let authorities: Vec<_> = pairs.iter().map(|sk| sk.public()).collect();

	ext.execute_with(|| {
		let tickets = make_tickets(tickets_count, &pair);
		let data = PreBuiltTickets { tickets, authorities };
		tickets_data_write(data);
	});
}
