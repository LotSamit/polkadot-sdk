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

//! Benchmarks for the Sassafras pallet.

use crate::*;
use sp_consensus_sassafras::EpochConfiguration;
use sp_std::vec;

use frame_benchmarking::v2::*;
use frame_system::RawOrigin;

const TICKETS_DATA: &[u8] = include_bytes!("../tickets.bin");

#[derive(Encode, Decode)]
struct PreBuiltTickets {
	authorities: Vec<AuthorityId>,
	tickets: Vec<TicketEnvelope>,
}

#[benchmarks]
mod benchmarks {
	use super::*;

	const LOG_TARGET: &str = "sassafras::benchmark";

	#[benchmark]
	fn submit_tickets(x: Linear<1, 20>) {
		let tickets_count = x as usize;

		let mut raw_data = TICKETS_DATA;
		let PreBuiltTickets { authorities, tickets } =
			PreBuiltTickets::decode(&mut raw_data).expect("Failed to decode tickets buffer");

		log::debug!(target: LOG_TARGET, "PreBuiltTickets: {} tickets, {} authorities", tickets.len(), authorities.len());

		Pallet::<T>::update_ring_verifier(&authorities);

		let next_config = EpochConfiguration { attempts_number: 20, redundancy_factor: u32::MAX };
		NextEpochConfig::<T>::set(Some(next_config));

		// Use the authorities in the pre-build tickets
		let authorities = WeakBoundedVec::force_from(authorities, None);
		NextAuthorities::<T>::set(authorities);

		let tickets = tickets[..tickets_count].to_vec();
		let tickets = BoundedVec::truncate_from(tickets);

		log::debug!(target: LOG_TARGET, "Submitting {} tickets", tickets_count);

		#[extrinsic_call]
		submit_tickets(RawOrigin::None, tickets);
	}

	#[benchmark]
	fn plan_config_change() {
		let config = EpochConfiguration { redundancy_factor: 1, attempts_number: 10 };

		#[extrinsic_call]
		plan_config_change(RawOrigin::Root, config);
	}

	// Construction of ring verifier benchmark
	#[benchmark]
	fn load_ring_context() {
		let ring_ctx = vrf::RingContext::new_testing();
		RingContext::<T>::set(Some(ring_ctx));

		#[block]
		{
			let _ring_ctx = RingContext::<T>::get().unwrap();
		}
	}

	// Construction of ring verifier benchmark
	#[benchmark]
	fn update_ring_verifier(x: Linear<1, 20>) {
		let authorities_count = x as usize;

		let ring_ctx = vrf::RingContext::new_testing();
		RingContext::<T>::set(Some(ring_ctx));

		let mut raw_data = TICKETS_DATA;
		let PreBuiltTickets { authorities, tickets: _ } =
			PreBuiltTickets::decode(&mut raw_data).expect("Failed to decode tickets buffer");
		let authorities: Vec<_> = authorities[..authorities_count].to_vec();

		#[block]
		{
			Pallet::<T>::update_ring_verifier(&authorities);
		}
	}

	// Tickets segments sorting function benchmark.
	#[benchmark]
	fn sort_segments(x: Linear<1, 100>) {
		use sp_consensus_sassafras::EphemeralPublic;
		let segments_count = x as u32;
		let tickets_count = segments_count * SEGMENT_MAX_SIZE;

		// Construct a bunch of dummy tickets
		let tickets: Vec<_> = (0..tickets_count)
			.map(|i| {
				let body = TicketBody {
					attempt_idx: i,
					erased_public: EphemeralPublic([i as u8; 32]),
					revealed_public: EphemeralPublic([i as u8; 32]),
				};
				let id_bytes = crate::hashing::blake2_128(&i.to_le_bytes());
				let id = TicketId::from_le_bytes(id_bytes);
				(id, body)
			})
			.collect();

		for (chunk_id, chunk) in tickets.chunks(SEGMENT_MAX_SIZE as usize).enumerate() {
			let segment: Vec<TicketId> = chunk
				.iter()
				.map(|(id, body)| {
					TicketsData::<T>::set(id, Some(body.clone()));
					*id
				})
				.collect();
			let segment = BoundedVec::truncate_from(segment);
			NextTicketsSegments::<T>::insert(chunk_id as u32, segment);
		}

		// Update metadata
		let mut meta = TicketsMeta::<T>::get();
		meta.unsorted_tickets_count = tickets_count;
		TicketsMeta::<T>::set(meta.clone());

		log::debug!(target: LOG_TARGET, "Before sort: {:?}", meta);
		#[block]
		{
			Pallet::<T>::sort_tickets(u32::MAX, 0, &mut meta);
		}
		log::debug!(target: LOG_TARGET, "After sort: {:?}", meta);
	}
}
