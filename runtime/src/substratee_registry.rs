/*
    Copyright 2019 Supercomputing Systems AG

    Licensed under the Apache License, Version 2.0 (the "License");
    you may not use this file except in compliance with the License.
    You may obtain a copy of the License at

        http://www.apache.org/licenses/LICENSE-2.0

    Unless required by applicable law or agreed to in writing, software
    distributed under the License is distributed on an "AS IS" BASIS,
    WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
    See the License for the specific language governing permissions and
    limitations under the License.

*/

use codec::{Decode, Encode};
use host_calls::runtime_interfaces::verify_ra_report;
use host_calls::SgxReport;
use primitives::H256;
use rstd::prelude::*;
use rstd::str;
use runtime_io::misc::print_utf8;
use support::{decl_event, decl_module, decl_storage, dispatch::Result, ensure, StorageLinkedMap};
use system::ensure_signed;

pub trait Trait: balances::Trait {
    type Event: From<Event<Self>> + Into<<Self as system::Trait>::Event>;
}

const MAX_RA_REPORT_LEN: usize = 4096;
const MAX_URL_LEN: usize = 256;

#[derive(Encode, Decode, Default, Copy, Clone, PartialEq)]
#[cfg_attr(feature = "std", derive(Debug))]
pub struct Enclave<PubKey, Url> {
    pub pubkey: PubKey, // FIXME: this is redundant information
    pub mr_enclave: [u8; 32],
    pub timestamp: i64, // unix epoch
    pub url: Url,       // utf8 encoded url
}

pub type ShardIdentifier = H256;

#[derive(Encode, Decode, Debug, Default, Clone, PartialEq, Eq)]
//#[cfg_attr(feature = "std", derive(Debug))]
pub struct Request {
    pub shard: ShardIdentifier,
    pub cyphertext: Vec<u8>,
}

decl_event!(
	pub enum Event<T>
	where
		<T as system::Trait>::AccountId,
	{
		AddedEnclave(AccountId, Vec<u8>),
		RemovedEnclave(AccountId),
		UpdatedIpfsHash(ShardIdentifier, u64, Vec<u8>),
		Forwarded(Request),
		CallConfirmed(AccountId, Vec<u8>),
	}
);

decl_storage! {
    trait Store for Module<T: Trait> as substraTEERegistry {
        // Simple lists are not supported in runtime modules as theoretically O(n)
        // operations can be executed while only being charged O(1), see substrate
        // Kitties tutorial Chapter 2, Tracking all Kitties.

        // watch out: we start indexing with 1 instead of zero in order to
        // avoid ambiguity between Null and 0
        pub EnclaveRegistry get(enclave): linked_map u64 => Enclave<T::AccountId, Vec<u8>>;
        pub EnclaveCount get(enclave_count): u64;
        pub EnclaveIndex get(enclave_index): map T::AccountId => u64;
        pub LatestIpfsHash get(latest_ipfs_hash) : map ShardIdentifier => Vec<u8>;
        // enclave index of the worker that recently committed an update
        pub WorkerForShard get(worker_for_shard) : map ShardIdentifier => u64;
    }
}

decl_module! {
    pub struct Module<T: Trait> for enum Call where origin: T::Origin {

         fn deposit_event() = default;

        // the substraTEE-worker wants to register his enclave
         pub fn register_enclave(origin, ra_report: Vec<u8>, ra_signer_attn: [u32; 16], worker_url: Vec<u8>) -> Result {
            print_utf8(b"substraTEE_registry: called into runtime call register_enclave()");
            let sender = ensure_signed(origin)?;
            ensure!(ra_report.len() <= MAX_RA_REPORT_LEN, "RA report too long");
            ensure!(worker_url.len() <= MAX_URL_LEN, "URL too long");
            print_utf8(b"substraTEE_registry: parameter lenght ok");
            match verify_ra_report(&ra_report, &ra_signer_attn.to_vec(), &sender.encode()) {
                Some(rep) => {
                    print_utf8(b"substraTEE_registry: host_call successful");
                    let report = SgxReport::decode(&mut &rep[..]).unwrap();
                    let enclave_signer = match T::AccountId::decode(&mut &report.pubkey[..]) {
                        Ok(signer) => signer,
                        Err(_) => return Err("failed to decode enclave signer")
                    };
                    print_utf8(b"substraTEE_registry: decoded signer");
                    // this is actually already implicitly tested by verify_ra_report
                    ensure!(sender == enclave_signer,
                        "extrinsic must be signed by attested enclave key");
                    print_utf8(b"substraTEE_registry: signer is a match");
                    // TODO: activate state checks as soon as we've fixed our setup
//                    ensure!((report.status == SgxStatus::Ok) | (report.status == SgxStatus::ConfigurationNeeded),
//                        "RA status is insufficient");
//                    print_utf8(b"substraTEE_registry: status is acceptable");
                    Self::register_verified_enclave(&sender, &report, worker_url.clone())?;
                    Self::deposit_event(RawEvent::AddedEnclave(sender, worker_url));
                    print_utf8(b"substraTEE_registry: enclave registered");
                    Ok(())

                }
                None => Err("Verifying RA report failed... returning")
            }
        }
        // TODO: we can't expect a dead enclave to unregister itself
        // alternative: allow anyone to unregister an enclave that hasn't recently supplied a RA
        // such a call should be feeless if successful
        pub fn unregister_enclave(origin) -> Result {
            let sender = ensure_signed(origin)?;

            Self::remove_enclave(&sender)?;
            Self::deposit_event(RawEvent::RemovedEnclave(sender));
            Ok(())
        }

        pub fn call_worker(origin, request: Request) -> Result {
            let _sender = ensure_signed(origin)?;
            Self::deposit_event(RawEvent::Forwarded(request));
            Ok(())
        }

        // the substraTEE-worker calls this function for every processed call to confirm a state update
         pub fn confirm_call(origin, shard: ShardIdentifier, call_hash: Vec<u8>, ipfs_hash: Vec<u8>) -> Result {
            let sender = ensure_signed(origin)?;
            ensure!(<EnclaveIndex<T>>::exists(&sender),
            "[SubstraTEERegistry]: IPFS state update requested by enclave that is not registered");
            let sender_index = Self::enclave_index(&sender);
            <LatestIpfsHash>::insert(shard, ipfs_hash.clone());
            <WorkerForShard>::insert(shard, sender_index);

            Self::deposit_event(RawEvent::CallConfirmed(sender, call_hash));
            Self::deposit_event(RawEvent::UpdatedIpfsHash(shard, sender_index, ipfs_hash));
            Ok(())
        }
    }
}

impl<T: Trait> Module<T> {
    fn register_verified_enclave(
        sender: &T::AccountId,
        report: &SgxReport,
        url: Vec<u8>,
    ) -> Result {
        let enclave = Enclave {
            pubkey: sender.clone(),
            mr_enclave: report.mr_enclave,
            timestamp: report.timestamp,
            url,
        };
        let enclave_idx = if <EnclaveIndex<T>>::exists(sender) {
            print_utf8(b"Updating already registered enclave");
            <EnclaveIndex<T>>::get(sender)
        } else {
            let enclaves_count = Self::enclave_count()
                .checked_add(1)
                .ok_or("[SubstraTEERegistry]: Overflow adding new enclave to registry")?;
            <EnclaveIndex<T>>::insert(sender, enclaves_count);
            <EnclaveCount>::put(enclaves_count);
            enclaves_count
        };

        <EnclaveRegistry<T>>::insert(enclave_idx, &enclave);
        Ok(())
    }

    fn remove_enclave(sender: &T::AccountId) -> Result {
        ensure!(
            <EnclaveIndex<T>>::exists(sender),
            "[SubstraTEERegistry]: Trying to remove an enclave that doesn't exist."
        );
        let index_to_remove = <EnclaveIndex<T>>::take(sender);

        let enclaves_count = Self::enclave_count();
        let new_enclaves_count = enclaves_count
            .checked_sub(1)
            .ok_or("[SubstraTEERegistry]: Underflow removing an enclave from the registry")?;

        Self::swap_and_pop(index_to_remove, new_enclaves_count + 1)?;
        <EnclaveCount>::put(new_enclaves_count);

        Ok(())
    }

    /// Our list implementation would introduce holes in out list if if we try to remove elements from the middle.
    /// As the order of the enclave entries is not important, we use the swap an pop method to remove elements from
    /// the registry.
    fn swap_and_pop(index_to_remove: u64, new_enclaves_count: u64) -> Result {
        if index_to_remove != new_enclaves_count {
            let last_enclave = <EnclaveRegistry<T>>::get(&new_enclaves_count);
            <EnclaveRegistry<T>>::insert(index_to_remove, &last_enclave);
            <EnclaveIndex<T>>::insert(last_enclave.pubkey, index_to_remove);
        }

        <EnclaveRegistry<T>>::remove(new_enclaves_count);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::substratee_registry;
    use externalities::set_and_run_with_externalities;
    use node_primitives::{AccountId, Signature};
    use primitives::{sr25519, Blake2Hasher, Pair, Public, H256};
    use sr_primitives::weights::Weight;
    use sr_primitives::{
        testing::Header,
        traits::{BlakeTwo256, IdentifyAccount, IdentityLookup, Verify},
        Perbill,
    };
    use std::{cell::RefCell, collections::HashSet};
    use support::traits::{Currency, FindAuthor, Get, LockIdentifier};
    use support::{assert_ok, impl_outer_event, impl_outer_origin, parameter_types};

    thread_local! {
        static EXISTENTIAL_DEPOSIT: RefCell<u64> = RefCell::new(0);
    }
    //pub type AccountId = u64;
    pub type BlockNumber = u64;
    pub type Balance = u64;
    pub struct ExistentialDeposit;
    impl Get<u64> for ExistentialDeposit {
        fn get() -> u64 {
            EXISTENTIAL_DEPOSIT.with(|v| *v.borrow())
        }
    }

    // reproduce with "substratee_worker dump_ra"
    const TEST1_CERT: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_cert_MRSIGNER1_MRENCLAVE1.der");
    const TEST2_CERT: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_cert_MRSIGNER2_MRENCLAVE2.der");
    const TEST3_CERT: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_cert_MRSIGNER3_MRENCLAVE2.der");
    const TEST1_SIGNER_ATTN: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_signer_attn_MRSIGNER1_MRENCLAVE1.bin");
    const TEST2_SIGNER_ATTN: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_signer_attn_MRSIGNER2_MRENCLAVE2.bin");
    const TEST3_SIGNER_ATTN: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_signer_attn_MRSIGNER3_MRENCLAVE2.bin");
    // reproduce with "substratee_worker getsignkey"
    const TEST1_SIGNER_PUB: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_signer_pubkey_MRSIGNER1_MRENCLAVE1.bin");
    const TEST2_SIGNER_PUB: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_signer_pubkey_MRSIGNER2_MRENCLAVE2.bin");
    const TEST3_SIGNER_PUB: &[u8] =
        include_bytes!("../../host_calls/test/test_ra_signer_pubkey_MRSIGNER3_MRENCLAVE2.bin");

    // reproduce with "make mrenclave" in worker repo root
    const TEST1_MRENCLAVE: [u8; 32] = [
        62, 252, 187, 232, 60, 135, 108, 204, 87, 78, 35, 169, 241, 237, 106, 217, 251, 241, 99,
        189, 138, 157, 86, 136, 77, 91, 93, 23, 192, 104, 140, 167,
    ];
    const TEST2_MRENCLAVE: [u8; 32] = [
        4, 190, 230, 132, 211, 129, 59, 237, 101, 78, 55, 174, 144, 177, 91, 134, 1, 240, 27, 174,
        81, 139, 8, 22, 32, 241, 228, 103, 189, 43, 44, 102,
    ];
    const TEST3_MRENCLAVE: [u8; 32] = [
        4, 190, 230, 132, 211, 129, 59, 237, 101, 78, 55, 174, 144, 177, 91, 134, 1, 240, 27, 174,
        81, 139, 8, 22, 32, 241, 228, 103, 189, 43, 44, 102,
    ];
    // unix epoch. must be later than this
    const TEST1_TIMESTAMP: i64 = 1580587262i64;
    const TEST2_TIMESTAMP: i64 = 1581259412i64;
    const TEST3_TIMESTAMP: i64 = 1581259975i64;

    //    const WASM_CODE: &'static [u8] = include_bytes!("../wasm/target/wasm32-unknown-unknown/release/substratee_node_runtime_wasm.compact.wasm");
    //const CERT: &[u8] = b"0\x82\x0c\x8c0\x82\x0c2\xa0\x03\x02\x01\x02\x02\x01\x010\n\x06\x08*\x86H\xce=\x04\x03\x020\x121\x100\x0e\x06\x03U\x04\x03\x0c\x07MesaTEE0\x1e\x17\r190617124609Z\x17\r190915124609Z0\x121\x100\x0e\x06\x03U\x04\x03\x0c\x07MesaTEE0Y0\x13\x06\x07*\x86H\xce=\x02\x01\x06\x08*\x86H\xce=\x03\x01\x07\x03B\0\x04RT\x16\x16 \xef_\xd8\xe7\xc3\xb7\x03\x1d\xd6:\x1fF\xe3\xf2b!\xa9/\x8b\xd4\x82\x8f\xd1\xff[\x9c\x97\xbc\xf27\xb8,L\x8a\x01\xb0r;;\xa9\x83\xdc\x86\x9f\x1d%y\xf4;I\xe4Y\xc80'$K[\xd6\xa3\x82\x0bw0\x82\x0bs0\x82\x0bo\x06\t`\x86H\x01\x86\xf8B\x01\r\x04\x82\x0b`{\"id\":\"117077750682263877593646412006783680848\",\"timestamp\":\"2019-06-17T12:46:04.002066\",\"version\":3,\"isvEnclaveQuoteStatus\":\"GROUP_OUT_OF_DATE\",\"platformInfoBlob\":\"1502006504000900000909020401800000000000000000000008000009000000020000000000000B401A355B313FC939B4F48A54349C914A32A3AE2C4871BFABF22E960C55635869FC66293A3D9B2D58ED96CA620B65D669A444C80291314EF691E896F664317CF80C\",\"isvEnclaveQuoteBody\":\"AgAAAEALAAAIAAcAAAAAAOE6wgoHKsZsnVWSrsWX9kky0kWt9K4xcan0fQ996Ct+CAj//wGAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABwAAAAAAAAAHAAAAAAAAAFJJYIbPVot9NzRCjW2z9+k+9K8BsHQKzVMEHOR14hNbAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACD1xnnferKFHD2uvYqTXdDA8iZ22kCD5xw7h38CMfOngAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAABSVBYWIO9f2OfDtwMd1jofRuPyYiGpL4vUgo/R/1ucl7zyN7gsTIoBsHI7O6mD3IafHSV59DtJ5FnIMCckS1vW\"}|EbPFH/ThUaS/dMZoDKC5EgmdUXUORFtQzF49Umi1P55oeESreJaUvmA0sg/ATSTn5t2e+e6ZoBQIUbLHjcWLMLzK4pJJUeHhok7EfVgoQ378i+eGR9v7ICNDGX7a1rroOe0s1OKxwo/0hid2KWvtAUBvf1BDkqlHy025IOiXWhXFLkb/qQwUZDWzrV4dooMfX5hfqJPi1q9s18SsdLPmhrGBheh9keazeCR9hiLhRO9TbnVgR9zJk43SPXW+pHkbNigW+2STpVAi5ugWaSwBOdK11ZjaEU1paVIpxQnlW1D6dj1Zc3LibMH+ly9ZGrbYtuJks4eRnjPhroPXxlJWpQ==|MIIEoTCCAwmgAwIBAgIJANEHdl0yo7CWMA0GCSqGSIb3DQEBCwUAMH4xCzAJBgNVBAYTAlVTMQswCQYDVQQIDAJDQTEUMBIGA1UEBwwLU2FudGEgQ2xhcmExGjAYBgNVBAoMEUludGVsIENvcnBvcmF0aW9uMTAwLgYDVQQDDCdJbnRlbCBTR1ggQXR0ZXN0YXRpb24gUmVwb3J0IFNpZ25pbmcgQ0EwHhcNMTYxMTIyMDkzNjU4WhcNMjYxMTIwMDkzNjU4WjB7MQswCQYDVQQGEwJVUzELMAkGA1UECAwCQ0ExFDASBgNVBAcMC1NhbnRhIENsYXJhMRowGAYDVQQKDBFJbnRlbCBDb3Jwb3JhdGlvbjEtMCsGA1UEAwwkSW50ZWwgU0dYIEF0dGVzdGF0aW9uIFJlcG9ydCBTaWduaW5nMIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEAqXot4OZuphR8nudFrAFiaGxxkgma/Es/BA+tbeCTUR106AL1ENcWA4FX3K+E9BBL0/7X5rj5nIgX/R/1ubhkKWw9gfqPG3KeAtIdcv/uTO1yXv50vqaPvE1CRChvzdS/ZEBqQ5oVvLTPZ3VEicQjlytKgN9cLnxbwtuvLUK7eyRPfJW/ksddOzP8VBBniolYnRCD2jrMRZ8nBM2ZWYwnXnwYeOAHV+W9tOhAImwRwKF/95yAsVwd21ryHMJBcGH70qLagZ7Ttyt++qO/6+KAXJuKwZqjRlEtSEz8gZQeFfVYgcwSfo96oSMAzVr7V0L6HSDLRnpb6xxmbPdqNol4tQIDAQABo4GkMIGhMB8GA1UdIwQYMBaAFHhDe3amfrzQr35CN+s1fDuHAVE8MA4GA1UdDwEB/wQEAwIGwDAMBgNVHRMBAf8EAjAAMGAGA1UdHwRZMFcwVaBToFGGT2h0dHA6Ly90cnVzdGVkc2VydmljZXMuaW50ZWwuY29tL2NvbnRlbnQvQ1JML1NHWC9BdHRlc3RhdGlvblJlcG9ydFNpZ25pbmdDQS5jcmwwDQYJKoZIhvcNAQELBQADggGBAGcIthtcK9IVRz4rRq+ZKE+7k50/OxUsmW8aavOzKb0iCx07YQ9rzi5nU73tME2yGRLzhSViFs/LpFa9lpQL6JL1aQwmDR74TxYGBAIi5f4I5TJoCCEqRHz91kpG6Uvyn2tLmnIdJbPE4vYvWLrtXXfFBSSPD4Afn7+3/XUggAlc7oCTizOfbbtOFlYA4g5KcYgS1J2ZAeMQqbUdZseZCcaZZZn65tdqee8UXZlDvx0+NdO0LR+5pFy+juM0wWbu59MvzcmTXbjsi7HY6zd53Yq5K244fwFHRQ8eOB0IWB+4PfM7FeAApZvlfqlKOlLcZL2uyVmzRkyR5yW72uo9mehX44CiPJ2fse9Y6eQtcfEhMPkmHXI01sN+KwPbpA39+xOsStjhP9N1Y1a2tQAVo+yVgLgV2Hws73Fc0o3wC78qPEA+v2aRs/Be3ZFDgDyghc/1fgU+7C+P6kbqd4poyb6IW8KCJbxfMJvkordNOgOUUxndPHEi/tb/U7uLjLOgPA==0\n\x06\x08*\x86H\xce=\x04\x03\x02\x03H\00E\x02!\0\xae6\x06\t@Sy\x8f\x8ec\x9d\xdci^Ex*\x92}\xdcG\x15A\x97\xd7\xd7\xd1\xccx\xe0\x1e\x08\x02 \x15Q\xa0BT\xde'~\xec\xbd\x027\xd3\xd8\x83\xf7\xe6Z\xc5H\xb4D\xf7\xe2\r\xa7\xe4^f\x10\x85p";
    const URL: &[u8] = &[
        119, 115, 58, 47, 47, 49, 50, 55, 46, 48, 46, 48, 46, 49, 58, 57, 57, 57, 49,
    ];

    #[derive(Clone, PartialEq, Eq, Debug)]
    pub struct TestRuntime;
    impl Trait for TestRuntime {
        type Event = TestEvent;
    }

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
        type Event = TestEvent;
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
        type Event = TestEvent;
        type TransferPayment = ();
        type DustRemoval = ();
        type ExistentialDeposit = ExistentialDeposit;
        type TransferFee = TransferFee;
        type CreationFee = CreationFee;
    }
    pub type Balances = balances::Module<TestRuntime>;

    type AccountPublic = <Signature as Verify>::Signer;

    // Easy access alias
    type Registry = super::Module<TestRuntime>;

    pub struct ExtBuilder;

    impl ExtBuilder {
        pub fn build() -> runtime_io::TestExternalities {
            let mut storage = system::GenesisConfig::default()
                .build_storage::<TestRuntime>()
                .unwrap();
            balances::GenesisConfig::<TestRuntime> {
                balances: vec![],
                vesting: vec![],
            }
            .assimilate_storage(&mut storage)
            .unwrap();
            runtime_io::TestExternalities::from(storage)
        }
    }

    mod generic_event {
        pub use super::super::Event;
    }

    impl_outer_event! {
        pub enum TestEvent for TestRuntime {
            generic_event<T>,
            balances<T>,
        }
    }

    pub type GenericEvent = Module<TestRuntime>;

    impl_outer_origin! {
        pub enum Origin for TestRuntime {}
    }

    //    Fixme:    Was not able to use these statics for the tests, always threw cannot move out of
    //              dereference of raw pointer. As copy trait not implemented for whatever reason.
    //    lazy_static! {
    //        #[derive(Clone, Copy, Encode, Decode, Default, PartialEq)]
    //        static ref ENC_1: Enclave<u64, Vec<u8>> = Enclave { pubkey: 10, url: URL.to_vec() };
    //        #[derive(Encode, Decode, Default, Clone, Copy, PartialEq)]
    //        static ref ENC_2: Enclave<u64, Vec<u8>> = Enclave { pubkey: 20, url: URL.to_vec() };
    //        #[derive(Encode, Decode, Default, Clone, Copy, PartialEq)]
    //        static ref ENC_3: Enclave<u64, Vec<u8>> = Enclave { pubkey: 30, url: URL.to_vec() };
    //    }

    fn get_signer1() -> (AccountId, [u32; 16]) {
        let signer_attn: [u32; 16] = Decode::decode(&mut TEST1_SIGNER_ATTN).unwrap();
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&TEST1_SIGNER_PUB[..32]);
        let signer: AccountId =
            AccountPublic::from(sr25519::Public::decode(&mut &TEST1_SIGNER_PUB[..]).unwrap())
                .into_account();

        (signer, signer_attn)
    }

    fn get_signer2() -> (AccountId, [u32; 16]) {
        let signer_attn: [u32; 16] = Decode::decode(&mut TEST2_SIGNER_ATTN).unwrap();
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&TEST2_SIGNER_PUB[..32]);
        let signer: AccountId =
            AccountPublic::from(sr25519::Public::decode(&mut &TEST2_SIGNER_PUB[..]).unwrap())
                .into_account();

        (signer, signer_attn)
    }

    fn get_signer3() -> (AccountId, [u32; 16]) {
        let signer_attn: [u32; 16] = Decode::decode(&mut TEST3_SIGNER_ATTN).unwrap();
        let mut pubkey = [0u8; 32];
        pubkey.copy_from_slice(&TEST3_SIGNER_PUB[..32]);
        let signer: AccountId =
            AccountPublic::from(sr25519::Public::decode(&mut &TEST3_SIGNER_PUB[..]).unwrap())
                .into_account();

        (signer, signer_attn)
    }

    fn list_enclaves() -> Vec<(u64, Enclave<AccountId, Vec<u8>>)> {
        <EnclaveRegistry<TestRuntime>>::enumerate()
            .collect::<Vec<(u64, Enclave<AccountId, Vec<u8>>)>>()
    }

    #[test]
    fn add_enclave_works() {
        ExtBuilder::build().execute_with(|| {
            let (signer, signer_attn) = get_signer1();
            assert_ok!(Registry::register_enclave(
                Origin::signed(signer),
                TEST1_CERT.to_vec(),
                signer_attn,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 1);
        })
    }

    #[test]
    fn add_and_remove_enclave_works() {
        ExtBuilder::build().execute_with(|| {
            let (signer, signer_attn) = get_signer1();
            assert_ok!(Registry::register_enclave(
                Origin::signed(signer.clone()),
                TEST1_CERT.to_vec(),
                signer_attn,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 1);
            assert_ok!(Registry::unregister_enclave(Origin::signed(signer)));
            assert_eq!(Registry::enclave_count(), 0);
            assert_eq!(list_enclaves(), vec![])
        })
    }

    #[test]
    fn list_enclaves_works() {
        ExtBuilder::build().execute_with(|| {
            let (signer, signer_attn) = get_signer1();
            let e_1: Enclave<AccountId, Vec<u8>> = Enclave {
                pubkey: signer.clone(),
                mr_enclave: TEST1_MRENCLAVE,
                timestamp: TEST1_TIMESTAMP,
                url: URL.to_vec(),
            };
            assert_ok!(Registry::register_enclave(
                Origin::signed(signer.clone()),
                TEST1_CERT.to_vec(),
                signer_attn,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 1);
            let enclaves = list_enclaves();
            assert_eq!(enclaves[0].1.pubkey, signer)
        })
    }

    #[test]
    fn remove_middle_enclave_works() {
        ExtBuilder::build().execute_with(|| {
            let (signer1, signer_attn1) = get_signer1();
            let (signer2, signer_attn2) = get_signer2();
            let (signer3, signer_attn3) = get_signer3();

            // add enclave 1
            let e_1: Enclave<AccountId, Vec<u8>> = Enclave {
                pubkey: signer1.clone(),
                mr_enclave: TEST1_MRENCLAVE,
                timestamp: TEST1_TIMESTAMP,
                url: URL.to_vec(),
            };

            let e_2: Enclave<AccountId, Vec<u8>> = Enclave {
                pubkey: signer2.clone(),
                mr_enclave: TEST2_MRENCLAVE,
                timestamp: TEST2_TIMESTAMP,
                url: URL.to_vec(),
            };

            let e_3: Enclave<AccountId, Vec<u8>> = Enclave {
                pubkey: signer3.clone(),
                mr_enclave: TEST3_MRENCLAVE,
                timestamp: TEST3_TIMESTAMP,
                url: URL.to_vec(),
            };

            assert_ok!(Registry::register_enclave(
                Origin::signed(signer1.clone()),
                TEST1_CERT.to_vec(),
                signer_attn1,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 1);
            assert_eq!(list_enclaves(), vec![(1, e_1.clone())]);

            // add enclave 2
            assert_ok!(Registry::register_enclave(
                Origin::signed(signer2.clone()),
                TEST2_CERT.to_vec(),
                signer_attn2,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 2);
            assert_eq!(list_enclaves(), vec![(2, e_2.clone()), (1, e_1.clone())]);

            // add enclave 3
            assert_ok!(Registry::register_enclave(
                Origin::signed(signer3.clone()),
                TEST3_CERT.to_vec(),
                signer_attn3,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 3);
            assert_eq!(
                list_enclaves(),
                vec![(3, e_3.clone()), (2, e_2.clone()), (1, e_1.clone())]
            );

            // remove enclave 2
            assert_ok!(Registry::unregister_enclave(Origin::signed(signer2)));
            assert_eq!(Registry::enclave_count(), 2);
            assert_eq!(list_enclaves(), vec![(2, e_3.clone()), (1, e_1.clone())]);
        })
    }

    #[test]
    fn register_invalid_enclave_fails() {
        let (signer, signer_attn) = get_signer1();
        assert!(
            Registry::register_enclave(
                Origin::signed(signer),
                Vec::new(),
                [0u32; 16],
                URL.to_vec()
            )
            .is_err(),
            URL.to_vec()
        );
    }

    #[test]
    fn update_enclave_url_works() {
        ExtBuilder::build().execute_with(|| {
            let (signer, signer_attn) = get_signer1();
            let url2 = "my fancy url".as_bytes();
            let e_1: Enclave<AccountId, Vec<u8>> = Enclave {
                pubkey: signer.clone(),
                mr_enclave: TEST1_MRENCLAVE,
                timestamp: TEST1_TIMESTAMP,
                url: url2.to_vec(),
            };

            assert_ok!(Registry::register_enclave(
                Origin::signed(signer.clone()),
                TEST1_CERT.to_vec(),
                signer_attn,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave(1).url, URL.to_vec());

            assert_ok!(Registry::register_enclave(
                Origin::signed(signer.clone()),
                TEST1_CERT.to_vec(),
                signer_attn,
                url2.to_vec()
            ));
            assert_eq!(Registry::enclave(1).url, url2.to_vec());
            let enclaves = list_enclaves();
            assert_eq!(enclaves[0].1.pubkey, signer)
        })
    }

    #[test]
    fn update_ipfs_hash_works() {
        ExtBuilder::build().execute_with(|| {
            let ipfs_hash = "QmYY9U7sQzBYe79tVfiMyJ4prEJoJRWCD8t85j9qjssS9y";
            let shard = H256::default();
            let request_hash = vec![];
            let (signer, signer_attn) = get_signer1();

            assert_ok!(Registry::register_enclave(
                Origin::signed(signer.clone()),
                TEST1_CERT.to_vec(),
                signer_attn,
                URL.to_vec()
            ));
            assert_eq!(Registry::enclave_count(), 1);
            assert_ok!(Registry::confirm_call(
                Origin::signed(signer.clone()),
                shard.clone(),
                request_hash.clone(),
                ipfs_hash.as_bytes().to_vec()
            ));
            assert_eq!(
                str::from_utf8(&Registry::latest_ipfs_hash(shard.clone())).unwrap(),
                ipfs_hash
            );
            assert_eq!(Registry::worker_for_shard(shard.clone()), 1u64);

            let expected_event = TestEvent::generic_event(RawEvent::UpdatedIpfsHash(
                shard.clone(),
                1,
                ipfs_hash.as_bytes().to_vec(),
            ));
            assert!(System::events().iter().any(|a| a.event == expected_event));

            let expected_event =
                TestEvent::generic_event(RawEvent::CallConfirmed(signer.clone(), request_hash));
            assert!(System::events().iter().any(|a| a.event == expected_event));
        })
    }

    #[test]
    fn ipfs_update_from_unregistered_enclave_fails() {
        ExtBuilder::build().execute_with(|| {
            let ipfs_hash = "QmYY9U7sQzBYe79tVfiMyJ4prEJoJRWCD8t85j9qjssS9y";
            let (signer, signer_attn) = get_signer1();
            assert!(Registry::confirm_call(
                Origin::signed(signer),
                H256::default(),
                vec![],
                ipfs_hash.as_bytes().to_vec()
            )
            .is_err());
        })
    }

    #[test]
    fn call_worker_works() {
        ExtBuilder::build().execute_with(|| {
            let req = Request {
                shard: ShardIdentifier::default(),
                cyphertext: vec![0u8, 1, 2, 3, 4],
            };
            let (signer, signer_attn) = get_signer1();
            assert!(Registry::call_worker(Origin::signed(signer), req.clone()).is_ok());
            let expected_event = TestEvent::generic_event(RawEvent::Forwarded(req));
            assert!(System::events().iter().any(|a| a.event == expected_event));
        })
    }
}
