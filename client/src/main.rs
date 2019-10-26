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

#![feature(rustc_private)]

#[macro_use]
extern crate clap;
#[macro_use] 
extern crate log;
extern crate env_logger;

use keyring::AccountKeyring;
use keystore::Store;
use substrate_api_client::{
    Api,
    compose_extrinsic_offline,
    extrinsic, 
    extrinsic::xt_primitives::{AccountId, UncheckedExtrinsicV3},
    rpc::json_req,
    utils::{storage_key_hash, hexstr_to_hash, hexstr_to_u256},
};
use codec::{Encode, Decode};
use primitives::{
	crypto::{set_default_ss58_version, Ss58AddressFormat, Ss58Codec},
	ed25519, sr25519, Pair, Public, H256, hexdisplay::HexDisplay,
};
use bip39::{Mnemonic, Language, MnemonicType};

use encointer_node_runtime::{Call, EncointerCeremoniesCall, BalancesCall, 
    Signature, Hash,
    encointer_ceremonies::{ClaimOfAttendance, Witness, CeremonyIndexType,
        MeetupIndexType}
}; 
use serde_json;
use log::{info, debug, trace, warn};
use log::Level;
use clap::App;

fn main() {
    env_logger::init();
    let yml = load_yaml!("cli.yml");
	let matches = App::from_yaml(yml).get_matches();

	let url = matches.value_of("URL").expect("must specify URL");
	info!("connecting to {}", url);
    let mut api = Api::<sr25519::Pair>::new(format!("ws://{}", url));
    let accountid = AccountId::from(AccountKeyring::Alice);
    let result_str = api
        .get_storage("Balances", "FreeBalance", Some(accountid.encode()))
        .unwrap();
    let result = hexstr_to_u256(result_str).unwrap();
    println!("[+] Alice's balance is {}", result);
    
    let keystore_path = "my_keystore";
	let keystore = Store::open(keystore_path, None).unwrap();


    if let Some(_matches) = matches.subcommand_matches("next_phase") {
        info!("will call next_phase() with extrinsic");
    }

    if let Some(_matches) = matches.subcommand_matches("get_balance") {
        let account = _matches.value_of("account").unwrap();
        let accountid: AccountId = match &account[..2] {
            "//" => sr25519::Pair::from_string(_matches.value_of("account").unwrap(), None).unwrap().public().into(),
            _ => sr25519::Public::from_ss58check(_matches.value_of("account").unwrap()).unwrap().into(),
        };
        let result_str = api
            .get_storage("Balances", "FreeBalance", Some(accountid.encode()))
            .unwrap();
        let result = hexstr_to_u256(result_str).unwrap();
        info!("ss58 is {}", accountid.to_ss58check());
        println!("[+] balance for {} is {}", account, result);
    }
}
