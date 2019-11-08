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

//! an RPC client to encointer node using websockets
//! 
//! examples
//! encointer-client 127.0.0.1:9944 transfer //Alice 5G9RtsTbiYJYQYMHbWfyPoeuuxNaCbC16tZ2JGrZ4gRKwz14 1000
//! 
#![feature(rustc_private)]

#[macro_use]
extern crate clap;
#[macro_use] 
extern crate log;
extern crate env_logger;

use keyring::AccountKeyring;
use keystore::Store;
use substrate_api_client::{
    Api, node_metadata,
    compose_extrinsic,
    extrinsic, 
    extrinsic::xt_primitives::{AccountId, UncheckedExtrinsicV3, GenericAddress},
    rpc::json_req,
    utils::{storage_key_hash, hexstr_to_hash, hexstr_to_u256, hexstr_to_u64, hexstr_to_vec},
};
use codec::{Encode, Decode};
use primitives::{
	crypto::{set_default_ss58_version, Ss58AddressFormat, Ss58Codec},
	ed25519, sr25519, Pair, Public, H256, hexdisplay::HexDisplay,
};
use bip39::{Mnemonic, Language, MnemonicType};

use encointer_node_runtime::{Event, Call, EncointerCeremoniesCall, BalancesCall, 
    Signature, Hash,
    encointer_ceremonies::{ClaimOfAttendance, Witness, CeremonyIndexType,
        MeetupIndexType, ParticipantIndexType}
}; 
//use primitive_types::U256;
use serde_json;
use log::{info, debug, trace, warn};
use log::Level;
use clap::App;
use std::sync::mpsc::channel;

fn main() {
    env_logger::init();
    let yml = load_yaml!("cli.yml");
	let matches = App::from_yaml(yml).get_matches();

	let url = matches.value_of("URL").expect("must specify URL");
	info!("connecting to {}", url);
    let api = Api::<sr25519::Pair>::new(format!("ws://{}", url));
    
    let keystore_path = "my_keystore";
	let keystore = Store::open(keystore_path, None).unwrap();

    if let Some(_matches) = matches.subcommand_matches("print_metadata") {
        let meta = api.get_metadata();
        println!(
            "Metadata:\n {}",
            node_metadata::pretty_format(&meta).unwrap()
        );
    }
    if let Some(_matches) = matches.subcommand_matches("listen") {
        info!("Subscribing to events");
        let (events_in, events_out) = channel();
        api.subscribe_events(events_in.clone());
        loop {
            let event_str = events_out.recv().unwrap();
            let _unhex = hexstr_to_vec(event_str).unwrap();
            let mut _er_enc = _unhex.as_slice();
            let _events = Vec::<system::EventRecord<Event, Hash>>::decode(&mut _er_enc);
            match _events {
                Ok(evts) => {
                    for evr in &evts {
                        debug!("decoded: phase {:?} event {:?}", evr.phase, evr.event);
                        match &evr.event {
/*                            Event::balances(be) => {
                                println!(">>>>>>>>>> balances event: {:?}", be);
                                match &be {
                                    balances::RawEvent::Transfer(transactor, dest, value, fee) => {
                                        println!("Transactor: {:?}", transactor);
                                        println!("Destination: {:?}", dest);
                                        println!("Value: {:?}", value);
                                        println!("Fee: {:?}", fee);
                                    }
                                    _ => {
                                        debug!("ignoring unsupported balances event");
                                    }
                                }
                            },*/
                            Event::encointer_ceremonies(ee) => {
                                println!(">>>>>>>>>> ceremony event: {:?}", ee);
                                match &ee {
                                    encointer_node_runtime::encointer_ceremonies::RawEvent::PhaseChangedTo(phase) => {
                                        println!("Phase changed to: {:?}", phase);
                                    },
                                    encointer_node_runtime::encointer_ceremonies::RawEvent::ParticipantRegistered(accountid) => {
                                        println!("Participant registered for ceremony: {:?}", accountid);
                                    },
                                    _ => {
                                        debug!("ignoring unsupported ceremony event");
                                    }
                                }
                            },
                            _ => debug!("ignoring unsupported module event: {:?}", evr.event),
                        }
                    }
                }
                Err(_) => error!("couldn't decode event record list"),
            }
        }
    }
 
    if let Some(_matches) = matches.subcommand_matches("get_balance") {
        let account = _matches.value_of("account").unwrap();
        let accountid = get_accountid_from_str(account);
        let result_str = api
            .get_storage("Balances", "FreeBalance", Some(accountid.encode()))
            .unwrap();
        let result = hexstr_to_u256(result_str).unwrap();
        info!("ss58 is {}", accountid.to_ss58check());
        println!("balance for {} is {}", account, result);
    }

    if let Some(_matches) = matches.subcommand_matches("transfer") {
        let arg_from = _matches.value_of("from").unwrap();
        let arg_to = _matches.value_of("to").unwrap();
        let amount = u128::from_str_radix(_matches.value_of("amount").unwrap(),10).expect("amount can be converted to u128");
        let from = get_accountid_from_str(arg_from);
        let to = get_accountid_from_str(arg_to);
        info!("from ss58 is {}", from.to_ss58check());
        info!("to ss58 is {}", to.to_ss58check());
        let _api = api.clone().set_signer(AccountKeyring::from_public(&from).unwrap().pair());
        let xt = _api.balance_transfer(GenericAddress::from(to.0.clone()), amount);
        let tx_hash = _api.send_extrinsic(xt.hex_encode()).unwrap();
        println!("[+] Transaction got finalized. Hash: {:?}\n", tx_hash);
        let result = _api.get_free_balance(&to);
        println!("balance for {} is now {}", to, result);
    }

    if let Some(_matches) = matches.subcommand_matches("next_phase") {
        let _api = api.clone().set_signer(AccountKeyring::Alice.pair());

        let xt: UncheckedExtrinsicV3<_, sr25519::Pair>  = compose_extrinsic!(
            _api.clone(),
            "EncointerCeremonies",
            "next_phase"
        );

        // send and watch extrinsic until finalized
        let tx_hash = _api.send_extrinsic(xt.hex_encode()).unwrap();
        println!("Transaction got finalized. Phase should've advanced. tx hash: {:?}", tx_hash);       
    }

    if let Some(_matches) = matches.subcommand_matches("register_participant") {
        let account = _matches.value_of("account").unwrap();
        let accountid = get_accountid_from_str(account);
        info!("ss58 is {}", accountid.to_ss58check());
        // FIXME: signer must be participant's Pair. now will always be Alice
        let _api = api.clone().set_signer(AccountKeyring::Alice.pair());

        let xt: UncheckedExtrinsicV3<_, sr25519::Pair>  = compose_extrinsic!(
            _api.clone(),
            "EncointerCeremonies",
            "register_participant"
        );

        // send and watch extrinsic until finalized
        let tx_hash = _api.send_extrinsic(xt.hex_encode()).unwrap();
        println!("Transaction got finalized. tx hash: {:?}", tx_hash);       

    }
    if let Some(_matches) = matches.subcommand_matches("list_meetup_registry") {
        let cindex = hexstr_to_u64(api
            .get_storage("EncointerCeremonies", "CurrentCeremonyIndex", None)
            .unwrap()
            ).unwrap() as CeremonyIndexType;
        println!("listing meetups for ceremony nr {}", cindex);
        let mcount = hexstr_to_u64(api
            .get_storage("EncointerCeremonies", "MeetupCount", None)
            .unwrap()
            ).unwrap() as MeetupIndexType;
        println!("number of meetups assigned:  {}", mcount);
        let res = api
            .get_storage_double_map("EncointerCeremonies", "MeetupRegistry", 
                cindex.encode(), 42u64.encode()).unwrap();
        let participants: Vec<AccountId> = Decode::decode(&mut &hexstr_to_vec(res).unwrap()[..]).unwrap();
        println!("MeetupRegistry[{}, {}]participants are:", cindex, 42);
        for p in participants.iter() {
            println!("   {:?}", p);
        }
        
    }

    if let Some(_matches) = matches.subcommand_matches("list_participant_registry") {
        let cindex = hexstr_to_u64(api
            .get_storage("EncointerCeremonies", "CurrentCeremonyIndex", None)
            .unwrap()
            ).unwrap() as CeremonyIndexType;
        println!("listing participants for ceremony nr {}", cindex);
        let pcount = hexstr_to_u64(api
            .get_storage("EncointerCeremonies", "ParticipantCount", None)
            .unwrap()
            ).unwrap() as ParticipantIndexType;
        println!("number of participants assigned:  {}", pcount);
        for p in 0..pcount {
            let res = api
                .get_storage_double_map("EncointerCeremonies", "ParticipantRegistry", 
                    cindex.encode(), p.encode()).unwrap();
            let accountid: AccountId = Decode::decode(&mut &hexstr_to_vec(res).unwrap()[..]).unwrap();
            println!("ParticipantRegistry[{}, {}] = {:?}", cindex, p, accountid);
        }
    }

    if let Some(_matches) = matches.subcommand_matches("list_witnesses_registry") {
        let cindex = hexstr_to_u64(api
            .get_storage("EncointerCeremonies", "CurrentCeremonyIndex", None)
            .unwrap()
            ).unwrap() as CeremonyIndexType;
        println!("listing witnesses for ceremony nr {}", cindex);
        let wcount = hexstr_to_u64(api
            .get_storage("EncointerCeremonies", "WitnessCount", None)
            .unwrap()
            ).unwrap() as ParticipantIndexType;
        println!("number of witnesses testimonials:  {}", wcount);
        for p in 0..wcount {
            let res = api
                .get_storage_double_map("EncointerCeremonies", "WitnessRegistry", 
                    cindex.encode(), p.encode()).unwrap();
            println!("WitnessRegistry[{}, {}] raw = {}", cindex, p, res);
            let witnesses: Vec<AccountId> = Decode::decode(&mut &hexstr_to_vec(res).unwrap()[..]).unwrap();
            println!("WitnessRegistry[{}, {}] = {:?}", cindex, p, witnesses);
        }
    }

}

fn get_accountid_from_str(account: &str) -> AccountId {
    match &account[..2] {
        "//" => sr25519::Pair::from_string(account, None).unwrap().public().into(),
        _ => sr25519::Public::from_ss58check(account).unwrap().into(),
    }
}