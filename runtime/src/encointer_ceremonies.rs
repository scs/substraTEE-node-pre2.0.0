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

use support::{decl_module, decl_storage, decl_event, ensure,
	storage::{StorageDoubleMap, StorageMap, StorageValue},
	traits::Currency,
	dispatch::Result};
use system::{ensure_signed, ensure_root};

use rstd::prelude::*;
use rstd::cmp::min;

use sr_primitives::traits::{Verify, Member, CheckedAdd, IdentifyAccount};
use sr_primitives::MultiSignature;
use runtime_io::misc::print_utf8;

use codec::{Codec, Encode, Decode};

#[cfg(feature = "std")]
use serde::{Serialize, Deserialize};

pub trait Trait: system::Trait + balances::Trait {
	type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
    type Public: IdentifyAccount<AccountId = Self::AccountId>;
    type Signature: Verify<Signer = Self::Public> + Member + Decode + Encode;
}

const SINGLE_MEETUP_INDEX: u64 = 1;

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

#[derive(Encode, Decode, Copy, Clone, PartialEq, Eq, Default, Debug)]
//#[cfg_attr(feature = "std", derive(Debug))]
pub struct Witness<Signature, AccountId> {
	pub claim: ClaimOfAttendance<AccountId>,
	pub signature: Signature,
	pub public: AccountId,
}

#[derive(Encode, Decode, Copy, Clone, PartialEq, Eq, Default, Debug)]
//#[cfg_attr(feature = "std", derive(Debug))]
pub struct ClaimOfAttendance<AccountId> {
	pub claimant_public: AccountId,
	pub ceremony_index: CeremonyIndexType,
	pub meetup_index: MeetupIndexType,
	pub number_of_participants_confirmed: u32,
}

// This module's storage items.
decl_storage! {
	trait Store for Module<T: Trait> as EncointerCeremonies {
		// everyone who registered for a ceremony
		// caution: index starts with 1, not 0! (because null and 0 is the same for state storage)
		ParticipantRegistry get(participant_registry): double_map CeremonyIndexType, blake2_256(ParticipantIndexType) => T::AccountId;
		ParticipantIndex get(participant_index): double_map CeremonyIndexType, blake2_256(T::AccountId) => ParticipantIndexType;
		ParticipantCount get(participant_count): ParticipantIndexType;

		// all meetups for each ceremony mapping to a vec of participants
		// caution: index starts with 1, not 0! (because null and 0 is the same for state storage)
		MeetupRegistry get(meetup_registry): double_map CeremonyIndexType, blake2_256(MeetupIndexType) => Vec<T::AccountId>;
		MeetupIndex get(meetup_index): double_map CeremonyIndexType, blake2_256(T::AccountId) => MeetupIndexType;
		MeetupCount get(meetup_count): MeetupIndexType;

		// collect fellow meetup participants accounts who witnessed key account
		// caution: index starts with 1, not 0! (because null and 0 is the same for state storage)
		WitnessRegistry get(witness_registry): double_map CeremonyIndexType, blake2_256(WitnessIndexType) => Vec<T::AccountId>;
		WitnessIndex get(witness_index): double_map CeremonyIndexType, blake2_256(T::AccountId) => WitnessIndexType;
		WitnessCount get(witness_count): WitnessIndexType;
		// how many peers does each participants observe at their meetup
		MeetupParticipantCountVote get(meetup_participant_count_vote): double_map CeremonyIndexType, blake2_256(T::AccountId) => u32;

		// caution: index starts with 1, not 0! (because null and 0 is the same for state storage)
		CurrentCeremonyIndex get(current_ceremony_index) config(): CeremonyIndexType;
		
		LastCeremonyBlock get(last_ceremony_block): T::BlockNumber;
		CurrentPhase get(current_phase): CeremonyPhaseType = CeremonyPhaseType::REGISTERING;

		CeremonyReward get(ceremony_reward) config(): T::Balance;
		CeremonyMaster get(ceremony_master) config(): T::AccountId;
	}
}

decl_module! {
	pub struct Module<T: Trait> for enum Call where origin: T::Origin {
		fn deposit_event() = default;

		pub fn next_phase(origin) -> Result {
			let sender = ensure_signed(origin)?;
			ensure!(sender == <CeremonyMaster<T>>::get(), "only the CeremonyMaster can call this function");
			let current_phase = <CurrentPhase>::get();
			let current_ceremony_index = <CurrentCeremonyIndex>::get();

			let next_phase = match current_phase {
				CeremonyPhaseType::REGISTERING => {
						Self::assign_meetups();
						CeremonyPhaseType::ASSIGNING
				},
				CeremonyPhaseType::ASSIGNING => {
						CeremonyPhaseType::WITNESSING
				},
				CeremonyPhaseType::WITNESSING => {
						Self::issue_rewards();
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
			print_utf8(b"phase changed");
			Ok(())
		}

		pub fn register_participant(origin) -> Result {
			let sender = ensure_signed(origin)?;
			ensure!(<CurrentPhase>::get() == CeremonyPhaseType::REGISTERING,
				"registering participants can only be done during REGISTERING phase");

			let cindex = <CurrentCeremonyIndex>::get();

			if <ParticipantIndex<T>>::exists(&cindex, &sender) {
				return Err("already registered participant")
			}

			let count = <ParticipantCount>::get();
			
			let new_count = count.checked_add(1).
            	ok_or("[EncointerCeremonies]: Overflow adding new participant to registry")?;
			
			<ParticipantRegistry<T>>::insert(&cindex, &new_count, &sender);
			<ParticipantIndex<T>>::insert(&cindex, &sender, &new_count);
			<ParticipantCount>::put(new_count);

			Ok(())
		}

		pub fn register_witnesses(origin, witnesses: Vec<Witness<T::Signature, T::AccountId>>) -> Result {
			let sender = ensure_signed(origin)?;
			ensure!(<CurrentPhase>::get() == CeremonyPhaseType::WITNESSING,			
				"registering witnesses can only be done during WITNESSING phase");
			let cindex = <CurrentCeremonyIndex>::get();
			let meetup_index = Self::meetup_index(&cindex, &sender);
			let mut meetup_participants = Self::meetup_registry(&cindex, &meetup_index);
			ensure!(meetup_participants.contains(&sender), "origin not part of this meetup");
			meetup_participants.remove_item(&sender);
			let num_registered = meetup_participants.len();
			let num_signed = witnesses.len();
			ensure!(num_signed <= num_registered, "can\'t have more witnesses than other meetup participants");
			let mut verified_witness_accounts = vec!();
			let mut claim_n_participants = 0u32;
			for w in 0..num_signed {
				let witness = &witnesses[w];
				let witness_account = &witnesses[w].public;
				if meetup_participants.contains(witness_account) == false { 
					print_utf8(b"ignoring witness that isn't a meetup participant");
					continue };
				if witness.claim.ceremony_index != cindex { 
					print_utf8(b"ignoring claim with wrong ceremony index");
					continue };
				if witness.claim.meetup_index != meetup_index { 
					print_utf8(b"ignoring claim with wrong meetup index");
					continue };
				if Self::verify_witness_signature(witness.clone()).is_err() { 
					print_utf8(b"ignoring witness with bad signature");
					continue };
				// witness is legit. insert it!
				verified_witness_accounts.insert(0, witness_account.clone());
				// is it a problem if this number isn't equal for all claims? Guess not.
				claim_n_participants = witness.claim.number_of_participants_confirmed;
			}
			if verified_witness_accounts.len() == 0 {
				return Err("no valid witnesses found");
			}

			let count = <WitnessCount>::get();
			let mut idx = count+1;

			if <WitnessIndex<T>>::exists(&cindex, &sender) {
				idx = <WitnessIndex<T>>::get(&cindex, &sender);
			} else {
				let new_count = count.checked_add(1).
            		ok_or("[EncointerCeremonies]: Overflow adding new witness to registry")?;
				<WitnessCount>::put(new_count);
			}
			<WitnessRegistry<T>>::insert(&cindex, &idx, &verified_witness_accounts);
			<WitnessIndex<T>>::insert(&cindex, &sender, &idx);
			<MeetupParticipantCountVote<T>>::insert(&cindex, &sender, &claim_n_participants);
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
		<MeetupRegistry<T>>::remove_prefix(&index);
		<MeetupIndex<T>>::remove_prefix(&index);
		<MeetupCount>::put(0);
		<WitnessRegistry<T>>::remove_prefix(&index);
		<WitnessIndex<T>>::remove_prefix(&index);
		<WitnessCount>::put(0);
		<MeetupParticipantCountVote<T>>::remove_prefix(&index);
		Ok(())
	}
	
	// this function is expensive, so it should later be processed off-chain within SubstraTEE-worker
	fn assign_meetups() -> Result {
		// for PoC1 we're assigning one single meetup with the first 12 participants only
		//ensure!(<CurrentPhase>::get() == CeremonyPhaseType::ASSIGNING,
		//		"registering meetups can only be done during ASSIGNING phase");
		let cindex = <CurrentCeremonyIndex>::get();		
		let pcount = <ParticipantCount>::get();		
		let mut meetup = vec!();
		
		for p in 1..min(pcount+1, 12+1) {
			let participant = <ParticipantRegistry<T>>::get(&cindex, &p);
			meetup.insert(meetup.len(), participant.clone());
			<MeetupIndex<T>>::insert(&cindex, &participant, &SINGLE_MEETUP_INDEX);
		}
		<MeetupRegistry<T>>::insert(&cindex, &SINGLE_MEETUP_INDEX, &meetup);
		<MeetupCount>::put(1);		
		Ok(())
	}

	fn verify_witness_signature(witness: Witness<T::Signature, T::AccountId>) -> Result {
		ensure!(witness.public != witness.claim.claimant_public, "witness may not be self-signed");
		match witness.signature.verify(&witness.claim.encode()[..], &witness.public) {
			true => Ok(()),
			false => Err("witness signature is invalid")
		}
	}

	// this function takes O(n) for n meetups, so it should later be processed off-chain within 
	// SubstraTEE-worker together with the entire registry
	// as this function can only be called by the ceremony state machine, it could actually work out fine
	// on-chain. It would just delay the next block once per ceremony cycle.
	fn issue_rewards() -> Result {
		ensure!(Self::current_phase() == CeremonyPhaseType::WITNESSING,			
			"issuance can only be called at the end of WITNESSING phase");
		let cindex = Self::current_ceremony_index();
		let meetup_count = Self::meetup_count();
		let reward = Self::ceremony_reward();		
		ensure!(meetup_count == 1, "registry must contain exactly one meetup for PoC1");

		for m in 0..meetup_count {
			// first, evaluate votes on how many participants showed up
			let (n_confirmed, n_honest_participants) = match Self::ballot_meetup_n_votes(SINGLE_MEETUP_INDEX) {
				Some(nn) => nn,
				_ => {
					print_utf8(b"skipping meetup because votes for num of participants are not dependable");
					continue;
				},
			};
			let mut meetup_participants = Self::meetup_registry(&cindex, &SINGLE_MEETUP_INDEX);
			for p in meetup_participants {
				if Self::meetup_participant_count_vote(&cindex, &p) != n_confirmed {
					print_utf8(b"skipped participant because of wrong participant count vote");
					continue; }
				let witnesses = Self::witness_registry(&cindex, 
					&Self::witness_index(&cindex, &p));
				if witnesses.len() < (n_honest_participants - 1) as usize || witnesses.is_empty() {
					print_utf8(b"skipped participant because of too few witnesses");
					continue; }
				let mut has_witnessed = 0u32;
				for w in witnesses {
					let w_witnesses = Self::witness_registry(&cindex, 
					&Self::witness_index(&cindex, &w));
					if w_witnesses.contains(&p) {
						has_witnessed += 1;
					}
				}
				if has_witnessed < (n_honest_participants - 1) {
					print_utf8(b"skipped participant because didn't testify for honest peers");
					continue; }					
				// TODO: check that p also signed others
				// participant merits reward
				print_utf8(b"participant merits reward");
				let old_balance = <balances::Module<T>>::free_balance(&p);
				let new_balance = old_balance.checked_add(&reward)
					.expect("Balance should never overflow");
				<balances::Module<T> as Currency<_>>::make_free_balance_be(&p, new_balance);
			}
		}
		Ok(())
	}

	fn ballot_meetup_n_votes(meetup_idx: MeetupIndexType) -> Option<(u32, u32)> {
		let cindex = Self::current_ceremony_index();
		let meetup_participants = Self::meetup_registry(&cindex, &meetup_idx);
		// first element is n, second the count of votes for n
		let mut n_vote_candidates: Vec<(u32,u32)> = vec!(); 
		for p in meetup_participants {
			let this_vote = match Self::meetup_participant_count_vote(&cindex, &p) {
				n if n > 0 => n,
				_ => continue,
			};
			match n_vote_candidates.iter().position(|&(n,c)| n == this_vote) {
				Some(idx) => n_vote_candidates[idx].1 += 1,
				_ => n_vote_candidates.insert(0, (this_vote,1)),
			};
		}
		if n_vote_candidates.is_empty() { return None; }
		// sort by descending vote count
		n_vote_candidates.sort_by(|a,b| b.1.cmp(&a.1));
		if n_vote_candidates[0].1 < 3 {
			return None;
		}
		Some(n_vote_candidates[0])
	}
}



/// tests for this module
#[cfg(test)]
mod tests {
	use super::*;
	use crate::encointer_ceremonies;
	use support::{impl_outer_event, impl_outer_origin, parameter_types, assert_ok};
	use sr_primitives::{Perbill, traits::{IdentityLookup, BlakeTwo256}, testing::Header};
	use std::{collections::HashSet, cell::RefCell};
	use externalities::set_and_run_with_externalities;
	use primitives::{H256, Blake2Hasher, Pair, Public, sr25519};
	use support::traits::{Currency, Get, FindAuthor, LockIdentifier};
	use sr_primitives::weights::Weight;
	use node_primitives::{AccountId, Signature};
	use test_client::{self, AccountKeyring};

	const NONE: u64 = 0;
	const REWARD: Balance = 1000;

	thread_local! {
		static EXISTENTIAL_DEPOSIT: RefCell<u64> = RefCell::new(0);
	}
	pub type BlockNumber = u64;
	pub type Balance = u64;

	type TestWitness = Witness<Signature, AccountId>;

	pub struct ExistentialDeposit;
	impl Get<u64> for ExistentialDeposit {
		fn get() -> u64 {
			EXISTENTIAL_DEPOSIT.with(|v| *v.borrow())
		}
	}

	#[derive(Clone, PartialEq, Eq, Debug)]
	pub struct TestRuntime;

	impl Trait for TestRuntime {
		type Event = ();
		type Public = <MultiSignature as Verify>::Signer;
		type Signature = MultiSignature;
	}
	
	pub type EncointerCeremonies = Module<TestRuntime>;

	parameter_types! {
		pub const BlockHashCount: u64 = 250;
		pub const MaximumBlockWeight: u32 = 1024;
		pub const MaximumBlockLength: u32 = 2 * 1024;
		pub const AvailableBlockRatio: Perbill = Perbill::one();
	}
	impl system::Trait for TestRuntime {
		type Origin = Origin;
		type Index = u64;
		type Call = ();
		type BlockNumber = BlockNumber;
        type Hash = H256;
        type Hashing = BlakeTwo256;
		type AccountId = AccountId;
		type Lookup = IdentityLookup<Self::AccountId>;
		type Header = Header;
		type Event = ();
		type BlockHashCount = BlockHashCount;
		type MaximumBlockWeight = MaximumBlockWeight;
		type MaximumBlockLength = MaximumBlockLength;
		type AvailableBlockRatio = AvailableBlockRatio;
		type Version = ();
	}

	pub type System = system::Module<TestRuntime>;

	parameter_types! {
		pub const TransferFee: Balance = 0;
		pub const CreationFee: Balance = 0;
		pub const TransactionBaseFee: u64 = 0;
		pub const TransactionByteFee: u64 = 0;
	}
	impl balances::Trait for TestRuntime {
		type Balance = Balance;
		type OnFreeBalanceZero = ();
		type OnNewAccount = ();
		type Event = ();
		type TransferPayment = ();
		type DustRemoval = ();
		type ExistentialDeposit = ExistentialDeposit;
		type TransferFee = TransferFee;
		type CreationFee = CreationFee;
	}
	pub type Balances = balances::Module<TestRuntime>;

	type AccountPublic = <Signature as Verify>::Signer;

	pub struct ExtBuilder;

	impl ExtBuilder {
		pub fn build() -> runtime_io::TestExternalities {
			let mut storage = system::GenesisConfig::default().build_storage::<TestRuntime>().unwrap();
			balances::GenesisConfig::<TestRuntime> {
				balances: vec![],
				vesting: vec![],
			}.assimilate_storage(&mut storage).unwrap();		
			encointer_ceremonies::GenesisConfig::<TestRuntime> {
				current_ceremony_index: 1,
				ceremony_reward: REWARD,
				ceremony_master: get_accountid(AccountKeyring::Alice),
			}.assimilate_storage(&mut storage).unwrap();		
			runtime_io::TestExternalities::from(storage)
		}
	}

	impl_outer_origin!{
		pub enum Origin for TestRuntime {}
	}

	fn meetup_claim_sign(claimant: AccountId, witness: AccountKeyring, n_participants: u32) -> TestWitness {
		let claim = ClaimOfAttendance {
			claimant_public: claimant.clone(),
			ceremony_index: 1,
			meetup_index: SINGLE_MEETUP_INDEX,
			number_of_participants_confirmed: n_participants,
		};
		TestWitness { 
			claim: claim.clone(),
			signature: Signature::from(witness.sign(&claim.encode())),
			public: get_accountid(witness),
		}
	}

	fn register_alice_bob_ferdie() {
		assert_ok!(EncointerCeremonies::register_participant(Origin::signed(get_accountid(AccountKeyring::Alice))));
		assert_ok!(EncointerCeremonies::register_participant(Origin::signed(get_accountid(AccountKeyring::Bob))));
		assert_ok!(EncointerCeremonies::register_participant(Origin::signed(get_accountid(AccountKeyring::Ferdie))));
	}

	fn register_charlie_dave_eve() {
		assert_ok!(EncointerCeremonies::register_participant(Origin::signed(get_accountid(AccountKeyring::Charlie))));
		assert_ok!(EncointerCeremonies::register_participant(Origin::signed(get_accountid(AccountKeyring::Dave))));
		assert_ok!(EncointerCeremonies::register_participant(Origin::signed(get_accountid(AccountKeyring::Eve))));
	}

	fn gets_witnessed_by(claimant: AccountId, witnesses: Vec<AccountKeyring>, n_participants: u32) {
		let mut testimonials: Vec<TestWitness> = vec!();
		for w in witnesses {
			testimonials.insert(0, 
				meetup_claim_sign(claimant.clone(), w.clone(), n_participants));
			
		}
		assert_ok!(EncointerCeremonies::register_witnesses(
				Origin::signed(claimant),
				testimonials.clone()));	
	}

	fn get_accountid(pair: AccountKeyring) -> AccountId {
		AccountPublic::from(pair.public()).into_account()
	}

	#[test]
	fn ceremony_phase_statemachine_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::REGISTERING);
			assert_eq!(EncointerCeremonies::current_ceremony_index(), 1);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::ASSIGNING);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::WITNESSING);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::REGISTERING);
			assert_eq!(EncointerCeremonies::current_ceremony_index(), 2);						
		});
	}

	#[test]
	fn registering_participant_works() {
		ExtBuilder::build().execute_with(|| {
			let alice = AccountId::from(AccountKeyring::Alice);
			let bob = AccountId::from(AccountKeyring::Bob);
			let cindex = EncointerCeremonies::current_ceremony_index();
			assert_eq!(EncointerCeremonies::participant_count(), 0);
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())));
			assert_eq!(EncointerCeremonies::participant_count(), 1);
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(bob.clone())));
			assert_eq!(EncointerCeremonies::participant_count(), 2);
			assert_eq!(EncointerCeremonies::participant_index(&cindex, &bob), 2);
			assert_eq!(EncointerCeremonies::participant_registry(&cindex, &1), alice);
			assert_eq!(EncointerCeremonies::participant_registry(&cindex, &2), bob);
		});
	}

	#[test]
	fn registering_participant_twice_fails() {
		ExtBuilder::build().execute_with(|| {
			let alice = AccountId::from(AccountKeyring::Alice);
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())));
			assert!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())).is_err());
		});
	}

	#[test]
	fn ceremony_index_and_purging_registry_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountId::from(AccountKeyring::Alice);
			let cindex = EncointerCeremonies::current_ceremony_index();
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())));
			assert_eq!(EncointerCeremonies::participant_registry(&cindex, &1), alice);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// now assigning
			assert_eq!(EncointerCeremonies::participant_registry(&cindex, &1), alice);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// now witnessing
			assert_eq!(EncointerCeremonies::participant_registry(&cindex, &1), alice);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// now again registering
			let new_cindex = EncointerCeremonies::current_ceremony_index();
			assert_eq!(new_cindex, cindex+1);
			assert_eq!(EncointerCeremonies::participant_count(), 0);
			assert_eq!(EncointerCeremonies::participant_registry(&cindex, &1), AccountId::default());
			assert_eq!(EncointerCeremonies::participant_index(&cindex, &alice), NONE);
		});
	}

	#[test]
	fn registering_participant_in_wrong_phase_fails() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountId::from(AccountKeyring::Alice);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_eq!(EncointerCeremonies::current_phase(), CeremonyPhaseType::ASSIGNING);
			assert!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())).is_err());
		});
	}

	#[test]
	fn assigning_meetup_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountId::from(AccountKeyring::Alice);
			let bob = AccountId::from(AccountKeyring::Bob);
			let ferdie = AccountId::from(AccountKeyring::Ferdie);
			let cindex = EncointerCeremonies::current_ceremony_index();
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())));
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(bob.clone())));
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(ferdie.clone())));
			assert_eq!(EncointerCeremonies::participant_count(), 3);
			//assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::assign_meetups());
			assert_eq!(EncointerCeremonies::meetup_count(), 1);
			let meetup = EncointerCeremonies::meetup_registry(&cindex, &SINGLE_MEETUP_INDEX);
			assert_eq!(meetup.len(), 3);
			assert!(meetup.contains(&alice));
			assert!(meetup.contains(&bob));
			assert!(meetup.contains(&ferdie));

			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &alice), SINGLE_MEETUP_INDEX);
			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &bob), SINGLE_MEETUP_INDEX);
			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &ferdie), SINGLE_MEETUP_INDEX);

		});
	}
	#[test]
	fn assigning_meetup_at_phase_change_and_purge_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountId::from(AccountKeyring::Alice);
			let cindex = EncointerCeremonies::current_ceremony_index();
			assert_ok!(EncointerCeremonies::register_participant(Origin::signed(alice.clone())));
			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &alice), NONE);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &alice), SINGLE_MEETUP_INDEX);
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &alice), NONE);
		});
	}

	#[test]
	fn verify_witness_signatue_works() {
		ExtBuilder::build().execute_with(|| {
			// claimant			
			let claimant = AccountKeyring::Alice;
			// witness
			let witness = AccountKeyring::Bob;

			let claim = ClaimOfAttendance {
				claimant_public: get_accountid(claimant),
				ceremony_index: 1,
				meetup_index: SINGLE_MEETUP_INDEX,
				number_of_participants_confirmed: 3,
			};
			let witness_good = TestWitness { 
				claim: claim.clone(),
				signature: Signature::from(witness.sign(&claim.encode())),
				public: get_accountid(witness),
			};
			let witness_wrong_signature = TestWitness { 
				claim: claim.clone(),
				signature: Signature::from(claimant.sign(&claim.encode())),
				public: get_accountid(witness),
			};
			let witness_wrong_signer = TestWitness { 
				claim: claim.clone(),
				signature: Signature::from(witness.sign(&claim.encode())),
				public: get_accountid(claimant),
			};
			assert_ok!(EncointerCeremonies::verify_witness_signature(witness_good));
			assert!(EncointerCeremonies::verify_witness_signature(witness_wrong_signature).is_err());
			assert!(EncointerCeremonies::verify_witness_signature(witness_wrong_signer).is_err());
		});
	}

	#[test]
	fn register_witnesses_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let ferdie = AccountKeyring::Ferdie;
			let cindex = EncointerCeremonies::current_ceremony_index();
			register_alice_bob_ferdie();
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			assert_eq!(EncointerCeremonies::meetup_index(&cindex, &get_accountid(alice)), SINGLE_MEETUP_INDEX);

			gets_witnessed_by(get_accountid(alice), vec!(bob,ferdie),3);
			gets_witnessed_by(get_accountid(bob), vec!(alice,ferdie),3);

			assert_eq!(EncointerCeremonies::witness_count(), 2);
			assert_eq!(EncointerCeremonies::witness_index(&cindex, &get_accountid(bob)), 2);
			let wit_vec = EncointerCeremonies::witness_registry(&cindex, &2);
			assert!(wit_vec.len() == 2);
			assert!(wit_vec.contains(&get_accountid(alice)));
			assert!(wit_vec.contains(&get_accountid(ferdie)));

			// TEST: re-registering must overwrite previous entry
			gets_witnessed_by(get_accountid(alice), vec!(bob,ferdie),3);
			assert_eq!(EncointerCeremonies::witness_count(), 2);	
		});
	}

	#[test]
	fn register_witnesses_for_non_participant_fails_silently() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let cindex = EncointerCeremonies::current_ceremony_index();
			register_alice_bob_ferdie();
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			gets_witnessed_by(get_accountid(alice), vec!(bob,alice),3);
			assert_eq!(EncointerCeremonies::witness_count(), 1);	
			let wit_vec = EncointerCeremonies::witness_registry(&cindex, &1);
			assert!(wit_vec.contains(&get_accountid(alice)) == false);
			assert!(wit_vec.len() == 1);

		});
	}

	#[test]
	fn register_witnesses_for_non_participant_fails() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let ferdie = AccountKeyring::Ferdie;
			let eve = AccountKeyring::Eve;
			let cindex = EncointerCeremonies::current_ceremony_index();
			register_alice_bob_ferdie();
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			let mut eve_witnesses: Vec<TestWitness> = vec!();
			eve_witnesses.insert(0, meetup_claim_sign(get_accountid(eve), alice.clone(),3));
			eve_witnesses.insert(1, meetup_claim_sign(get_accountid(eve), ferdie.clone(),3));
			assert!(EncointerCeremonies::register_witnesses(
				Origin::signed(get_accountid(eve)),
				eve_witnesses.clone())
				.is_err());

		});
	}

	#[test]
	fn register_witnesses_with_non_participant_fails_silently() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let eve = AccountKeyring::Eve;
			let cindex = EncointerCeremonies::current_ceremony_index();
			register_alice_bob_ferdie();
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			gets_witnessed_by(get_accountid(alice), vec!(bob, eve), 3);
			assert_eq!(EncointerCeremonies::witness_count(), 1);	
			let wit_vec = EncointerCeremonies::witness_registry(&cindex, &1);
			assert!(wit_vec.contains(&get_accountid(eve)) == false);
			assert!(wit_vec.len() == 1);			
		});
	}

	#[test]
	fn register_witnesses_with_wrong_meetup_index_fails() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let ferdie = AccountKeyring::Ferdie;
			let cindex = EncointerCeremonies::current_ceremony_index();
			register_alice_bob_ferdie();
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			let mut alice_witnesses: Vec<TestWitness> = vec!();
			alice_witnesses.insert(0, meetup_claim_sign(get_accountid(alice), bob.clone(), 3));
			let claim = ClaimOfAttendance {
				claimant_public: get_accountid(alice),
				ceremony_index: 1,
				// !!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!
				meetup_index: SINGLE_MEETUP_INDEX + 99,
				number_of_participants_confirmed: 3,
			};
			alice_witnesses.insert(1, 
				TestWitness { 
					claim: claim.clone(),
					signature: Signature::from(ferdie.sign(&claim.encode())),
					public: get_accountid(ferdie),
				}
			);
			assert_ok!(EncointerCeremonies::register_witnesses(
				Origin::signed(get_accountid(alice)),
				alice_witnesses));
			let wit_vec = EncointerCeremonies::witness_registry(&cindex, &1);
			assert!(wit_vec.contains(&get_accountid(ferdie)) == false);
			assert!(wit_vec.len() == 1);			
		});
	}

	#[test]
	fn register_witnesses_with_wrong_ceremony_index_fails() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let ferdie = AccountKeyring::Ferdie;
			let cindex = EncointerCeremonies::current_ceremony_index();
			register_alice_bob_ferdie();
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			let mut alice_witnesses: Vec<TestWitness> = vec!();
			alice_witnesses.insert(0, meetup_claim_sign(get_accountid(alice), bob.clone(), 3));
			let claim = ClaimOfAttendance {
				claimant_public: get_accountid(alice),
				// !!!!!!!!!!!!!!!!!!!!!!!!!!
				ceremony_index: 99,
				meetup_index: SINGLE_MEETUP_INDEX,
				number_of_participants_confirmed: 3,
			};
			alice_witnesses.insert(1, 
				TestWitness { 
					claim: claim.clone(),
					signature: Signature::from(ferdie.sign(&claim.encode())),
					public: get_accountid(ferdie),
				}
			);
			assert_ok!(EncointerCeremonies::register_witnesses(
				Origin::signed(get_accountid(alice)),
				alice_witnesses));
			let wit_vec = EncointerCeremonies::witness_registry(&cindex, &1);
			assert!(wit_vec.contains(&get_accountid(ferdie)) == false);
			assert!(wit_vec.len() == 1);			
		});
	}


	#[test]
	fn ballot_meetup_n_votes_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let ferdie = AccountKeyring::Ferdie;
			let charlie = AccountKeyring::Charlie;
			let dave = AccountKeyring::Dave;
			let eve = AccountKeyring::Eve;
			let cindex = EncointerCeremonies::current_ceremony_index();			
			register_alice_bob_ferdie();
			register_charlie_dave_eve();

			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// ASSIGNING
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			gets_witnessed_by(get_accountid(alice), vec!(bob),5);
			gets_witnessed_by(get_accountid(bob), vec!(alice),5);
			gets_witnessed_by(get_accountid(charlie), vec!(alice),5);
			gets_witnessed_by(get_accountid(dave), vec!(alice),5);
			gets_witnessed_by(get_accountid(eve), vec!(alice),5);
			gets_witnessed_by(get_accountid(ferdie), vec!(dave),6);
			assert!(EncointerCeremonies::ballot_meetup_n_votes(SINGLE_MEETUP_INDEX) == Some((5,5)));

			gets_witnessed_by(get_accountid(alice), vec!(bob),5);
			gets_witnessed_by(get_accountid(bob), vec!(alice),5);
			gets_witnessed_by(get_accountid(charlie), vec!(alice),4);
			gets_witnessed_by(get_accountid(dave), vec!(alice),4);
			gets_witnessed_by(get_accountid(eve), vec!(alice),6);
			gets_witnessed_by(get_accountid(ferdie), vec!(dave),6);
			assert!(EncointerCeremonies::ballot_meetup_n_votes(SINGLE_MEETUP_INDEX) == None);

			gets_witnessed_by(get_accountid(alice), vec!(bob),5);
			gets_witnessed_by(get_accountid(bob), vec!(alice),5);
			gets_witnessed_by(get_accountid(charlie), vec!(alice),5);
			gets_witnessed_by(get_accountid(dave), vec!(alice),4);
			gets_witnessed_by(get_accountid(eve), vec!(alice),6);
			gets_witnessed_by(get_accountid(ferdie), vec!(dave),6);
			assert!(EncointerCeremonies::ballot_meetup_n_votes(SINGLE_MEETUP_INDEX) == Some((5,3)));
		});
	}

	#[test]
	fn issue_reward_works() {
		ExtBuilder::build().execute_with(|| {
			let master = AccountId::from(AccountKeyring::Alice);
			let alice = AccountKeyring::Alice;
			let bob = AccountKeyring::Bob;
			let ferdie = AccountKeyring::Ferdie;
			let charlie = AccountKeyring::Charlie;
			let dave = AccountKeyring::Dave;
			let eve = AccountKeyring::Eve;
			let cindex = EncointerCeremonies::current_ceremony_index();			
			register_alice_bob_ferdie();
			register_charlie_dave_eve();

			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// ASSIGNING
			assert_ok!(EncointerCeremonies::next_phase(Origin::signed(master.clone())));
			// WITNESSING
			// ferdi doesn't show up
			// eve signs no one else
			// charlie collects incomplete signatures
			// dave signs ferdi and reports wrong number of participants
			gets_witnessed_by(get_accountid(alice), vec!(bob,charlie,dave),5);
			gets_witnessed_by(get_accountid(bob), vec!(alice,charlie,dave),5);
			gets_witnessed_by(get_accountid(charlie), vec!(alice,bob),5);
			gets_witnessed_by(get_accountid(dave), vec!(alice,bob,charlie),6);
			gets_witnessed_by(get_accountid(eve), vec!(alice,bob,charlie,dave),5);
			gets_witnessed_by(get_accountid(ferdie), vec!(dave),6);
			assert_eq!(Balances::free_balance(&get_accountid(alice)), 0);

			assert_ok!(EncointerCeremonies::issue_rewards());

			assert_eq!(Balances::free_balance(&get_accountid(alice)), REWARD);
			assert_eq!(Balances::free_balance(&get_accountid(bob)), REWARD);
			assert_eq!(Balances::free_balance(&get_accountid(charlie)), 0);
			assert_eq!(Balances::free_balance(&get_accountid(eve)), 0);
			assert_eq!(Balances::free_balance(&get_accountid(ferdie)), 0);
		});
	}
}
