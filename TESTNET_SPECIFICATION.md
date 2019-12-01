# Encointer "Cantillon" Testnet Specification
This document specifies a first testnet for encointer with the working title "Cantillon". 

## Key Differences with [Encointer Whitepaper](https://github.com/encointer/whitepaper)
  * no dPOET consensus. 
    * This testnet shall become a parachain to [Kusama](https://kusama.network/) based on [substrate](https://substrate.dev)
    * no decentralization. The Parachain will feature PoA consensus with Kusama as root-of-trust.
  * no proportional fees. Classical per-transaction base fees will be applied
  * confidential state updates will be implemented with [SubstraTEE](https://github.com/scs/substraTEE)
  * no private smart contracts yet
  * in addition to ceremonies happening every 41 days, there will be daily rehearsal ceremonies that allow to practise and demonstrate meetups
  
