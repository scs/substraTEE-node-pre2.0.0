# Adversary Model
We categorize users into the following psychological profiles for later scenario analysis:

## Fundamental Profiles
### Honest <img src=./emoji_innocent_1F607.svg width=64> 

Will comply with the rules

### Greedy <img src=./emoji_devil_1F47F.svg width=64> 

Will try to cheat the system for his/her own benefit. Will collude with others

Strategies:
  * Get reward without showing up
  * Sign sybil id's to collect more rewards

### Saboteur <img src=./emoji_angry_1F621.svg width=64> 

Will try to hurt the system, even if this comes at a cost. Will collude with others

Strategies:
  * turn meetups invalid
  * demoralize other participants by preventing them to get reward


## More Roles

### Sybil <img src=./emoji_alien_1F47D.svg width=64>

An identity that has no bijective relationship with a person

### Manipulable <img src=./emoji_hearts_1F970.svg width=64>

An honest person that can be convinced to break the rules by social pressure. Will not strive for economic benefit.

## Collusion Organizations

### evil.corp
An organization of the greedy or saboteur participants. 

Strategies: 
* undermining randomization by sharing information and key pairs to allow collusion attacks at meetups

# Threat Model
We assume 

1. The majority of ceremony participants is honest
2. At least one honest registered participant is present at every meetup which has some reputation from previous ceremonies

# Rule Design

When designing rules there's a tradeoff beween preventing either the greedy or the saboteur's success. We can introduce very restrictive rules that will successfully prevent sybil attacks by the greedy, but these will make it very easy for the saboteur to demoralize participants by turning meetups invalid deliberately.


# Behavioural Meetup Scenario Analysis
The scenario analysis is structured by the number of participants who were assigned to a meetup

## 3 Registered Participants

### Happy Flow
All participants only sign for persons present at meetup location. 

![](./meetup_3r_3i.svg)

Noshow of one is treated with mercy for attendees

![](./meetup_3r_2i1s_1noshow.svg)

### Attacks

#### Social Engineering
Adversary B talks A into signing his sybil ID

![](./meetup_3r_1m1d1a.svg)

As there is no honest participant in this meetup, this attack is out of scope of our threat model.

Mitigation
* randomized meetups should reduce the chance that someone could be assigned with his sybil to the same meetup. In weakly populated areas, this can happen easily though

#### Exclusion

Adversary B refuses to sign A and signs C instead

![](./meetup_3r_1i1d1a.svg)

A variant of this would be that B isn't even present at the meetup

Mitigation
* randomized meetups should reduce the chance that someone could be assigned with his sybil to the same meetup. In weakly populated areas, this can happen easily though. evil.corp could also increase chances.
* Only allow meetups with more participants (>=4?) in order to reduce the impact of a single participant on outcome.

## 4 Registered Participants

### Oversigning

![](./meetup_4r_1i2d1a.svg)

Mitigation: 
  1. Introduce **Reputation** (previous attendance to successful meetups)
  2. Introduce Rule *"lowest vote with reputation wins"*
    * This rule, however, will cause B, C, and D to vote "3" showups and to refuse to sign A. Reputation doesn't help because B and C could have reputation as well.
  3. invalidate non-consistent meetups

At this meetup, 3/4 of registered participants are malicious.

### Location Spoofing

![](./meetup_4r_2i1d1a.svg)

C and D could pretend to be at the meetup location. The ceremony validation has no way to know if A-B is legit or C-D.

Variants:
  * D could be a real person colluding with C

Probability:
  * low because of randomization if number of meetups assigned is high
    * evil.corp could redistribute sybil id's after meeting assignments, undermining randomization

Mitigation: 
  1. track participant locations and require plausible movement trajectories before meetup. **not sound** because it is easy for C+D to spoof realistic trajectories
  2. Quarantine: The validator can detect that the meetup is split and therefore not consistent
  3. invalidate non-consistent meetups, preventing 2 illegit rewards while demoralizing 2 honest participants.

At this meetup, 1/2 of registered participants are malicious.