//  Copyright (c) 2019 Alain Brenzikofer
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//       http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.

use support::{decl_module, decl_storage, decl_event, 
	storage::{StorageDoubleMap, StorageMap, StorageValue},
	dispatch::Result};
use system::{ensure_signed, ensure_root};
use rstd::vec::Vec;

use codec::{Codec, Encode, Decode};

#[cfg(feature = "std")]
use serde::{Serialize, Deserialize};

/// The module's configuration trait.
pub trait Trait: system::Trait + balances::Trait {
	// TODO: Add other types and constants required configure this module.

	/// The overarching event type.
	type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
}

pub type CeremonyIndexType = u32;
pub type ParticipantIndexType = u64;
pub type MeetupIndexType = u64;
pub type WitnessIndexType = u64;

#[derive(Encode, Decode, Copy, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "std", derive(Serialize, Deserialize, Debug))]
pub enum CeremonyPhaseType {
	REGISTERING, 
	ASSIGNING,
	WITNESSING, 	
}
impl Default for CeremonyPhaseType {
    fn default() -> Self { CeremonyPhaseType::REGISTERING }
}

// This module's storage items.
decl_storage! {
	trait Store for Module<T: Trait> as EncointerCeremonies {
		// everyone who registered for a ceremony
		ParticipantRegistry: double_map CeremonyIndexType, twox_128(ParticipantIndexType) => T::AccountId;
		ParticipantIndex: double_map CeremonyIndexType, twox_128(T::AccountId) => ParticipantIndexType;
		ParticipantCount get(participant_count): ParticipantIndexType;

		// all meetups for each ceremony mapping to a vec of participants
		MeetupRegistry: double_map CeremonyIndexType, twox_128(MeetupIndexType) => Vec<T::AccountId>;
		MeetupIndex: double_map CeremonyIndexType, twox_128(T::AccountId) => MeetupIndexType;
		MeetupCount get(meetup_count): MeetupIndexType;

		// collect fellow meetup participants accounts who witnessed key account
		WitnessRegistry: double_map CeremonyIndexType, twox_128(WitnessIndexType) => Vec<T::AccountId>;
		WitnessIndex: double_map CeremonyIndexType, twox_128(T::AccountId) => WitnessIndexType;
		WitnessCount get(witness_count): WitnessIndexType;

		CurrentCeremonyIndex get(current_ceremony_index): CeremonyIndexType;
		LastCeremonyBlock get(last_ceremony_block): T::BlockNumber;
		CurrentPhase get(current_phase): CeremonyPhaseType = CeremonyPhaseType::REGISTERING;

		CeremonyReward get(ceremony_reward) config(): T::Balance;
	}
}

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		fn deposit_event() = default;

		fn next_phase(origin) -> Result {
			ensure_root(origin)?;
			let current_phase = <CurrentPhase>::get();
			let current_ceremony_index = <CurrentCeremonyIndex>::get();

			let next_phase = match current_phase {
				CeremonyPhaseType::REGISTERING => {
						CeremonyPhaseType::ASSIGNING
				},
				CeremonyPhaseType::ASSIGNING => {
						CeremonyPhaseType::WITNESSING
				},
				CeremonyPhaseType::WITNESSING => {
						let next_ceremony_index = match current_ceremony_index.checked_add(1) {
							Some(v) => v,
							None => 0, //deliberate wraparound
						};
						Self::purge_registry(current_ceremony_index);
						<CurrentCeremonyIndex>::put(next_ceremony_index);									
						CeremonyPhaseType::REGISTERING
				},
			};

			<CurrentPhase>::put(next_phase);
			Self::deposit_event(RawEvent::PhaseChangedTo(next_phase));
			Ok(())
		}

		fn register_participant(origin) -> Result {
			ensure_signed(origin)?;
			let count = <ParticipantCount>::get();
			let new_count = match count.checked_add(1) {
							Some(v) => v,
							None => return Err("got overflow after adding one more participant"),
						};
			<ParticipantCount>::put(new_count);
			Ok(())
		}

	}
}

decl_event!(
	pub enum Event<T> where AccountId = <T as system::Trait>::AccountId {
		PhaseChangedTo(CeremonyPhaseType),
		ParticipantRegistered(AccountId),
	}
);


impl<T: Trait> Module<T> {
	fn purge_registry(index: CeremonyIndexType) -> Result {
		<ParticipantRegistry<T>>::remove_prefix(&index);
		<ParticipantIndex<T>>::remove_prefix(&index);
		<ParticipantCount>::put(0);
		Ok(())
	}
}



/// tests for this module
#[cfg(test)]
mod tests {
	use super::*;
	
	use std::{collections::HashSet, cell::RefCell};
	use runtime_io::with_externalities;
	use primitives::{H256, Blake2Hasher};
	use support::{impl_outer_origin, assert_ok, parameter_types};
	use support::traits::{Currency, Get, FindAuthor, LockIdentifier};
	use runtime_primitives::{traits::{BlakeTwo256, IdentityLookup}, testing::Header};
	use runtime_primitives::weights::Weight;
	use runtime_primitives::Perbill;

	const ID_1: LockIdentifier = *b"1       ";
	const ID_2: LockIdentifier = *b"2       ";
	const ID_3: LockIdentifier = *b"3       ";
	
	thread_local! {
		static EXISTENTIAL_DEPOSIT: RefCell<u64> = RefCell::new(0);
	}
	pub type AccountId = u64;
	pub type BlockNumber = u64;
	pub type Balance = u64;
	pub struct ExistentialDeposit;
	impl Get<u64> for ExistentialDeposit {
		fn get() -> u64 {
			EXISTENTIAL_DEPOSIT.with(|v| *v.borrow())
		}
	}


	impl_outer_origin! {
		pub enum Origin for Test {}
	}

	// For testing the module, we construct most of a mock runtime. This means
	// first constructing a configuration type (`Test`) which `impl`s each of the
	// configuration traits of modules we want to use.
	#[derive(Clone, Eq, PartialEq)]
	pub struct Test;
	parameter_types! {
		pub const BlockHashCount: u64 = 250;
		pub const MaximumBlockWeight: Weight = 1024;
		pub const MaximumBlockLength: u32 = 2 * 1024;
		pub const AvailableBlockRatio: Perbill = Perbill::from_percent(75);
	}
	impl system::Trait for Test {
		type Origin = Origin;
		type Call = ();
		type Index = u64;
		type BlockNumber = u64;
		type Hash = H256;
		type Hashing = BlakeTwo256;
		type AccountId = u64;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Header = Header;
		type WeightMultiplierUpdate = ();
		type Event = ();
		type BlockHashCount = BlockHashCount;
		type MaximumBlockWeight = MaximumBlockWeight;
		type MaximumBlockLength = MaximumBlockLength;
		type AvailableBlockRatio = AvailableBlockRatio;
		type Version = ();
	}
	parameter_types! {
		pub const TransferFee: Balance = 0;
		pub const CreationFee: Balance = 0;
		pub const TransactionBaseFee: u64 = 0;
		pub const TransactionByteFee: u64 = 0;
	}
	impl balances::Trait for Test {
		type Balance = Balance;
		type OnFreeBalanceZero = ();
		type OnNewAccount = ();
		type Event = ();
		type TransactionPayment = ();
		type TransferPayment = ();
		type DustRemoval = ();
		type ExistentialDeposit = ExistentialDeposit;
		type TransferFee = TransferFee;
		type CreationFee = CreationFee;
		type TransactionBaseFee = TransactionBaseFee;
		type TransactionByteFee = TransactionByteFee;
		type WeightToFee = ();
	}

	impl Trait for Test {
		type Event = ();
	}

	type EncointerCeremonies = Module<Test>;

	// This function basically just builds a genesis storage key/value store according to
	// our desired mockup.
	fn new_test_ext() -> runtime_io::TestExternalities<Blake2Hasher> {
		system::GenesisConfig::default().build_storage::<Test>().unwrap().into()
	}

	#[test]
	fn ceremony_phase_statemachine_works() {
		with_externalities(&mut new_test_ext(), || {

			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::REGISTERING);
			assert_eq!(EncointerCeremonies::current_ceremony_index(), 0);
			assert_ok!(EncointerCeremonies::next_phase(Origin::ROOT));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::ASSIGNING);
			assert_ok!(EncointerCeremonies::next_phase(Origin::ROOT));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::WITNESSING);
			assert_ok!(EncointerCeremonies::next_phase(Origin::ROOT));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::REGISTERING);
			assert_eq!(EncointerCeremonies::current_ceremony_index(), 1);						
		});
	}

	#[test]
	fn registering_participant_works() {
		with_externalities(&mut new_test_ext(), || {
			assert_eq!(EncointerCeremonies::participant_count(), 0);
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(1)));
			assert_eq!(EncointerCeremonies::participant_count(), 1);

		});
	}

}
