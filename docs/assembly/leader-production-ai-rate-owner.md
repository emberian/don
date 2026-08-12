# `production_ai_setup` planning-rate owner and executable child

`LeaderDataEncrypt::rate[6]` is now represented by an explicit decoded canonical owner rather
than an alias of either displayed income or `LeaderData::base_rate`. The executable boundary is
`crates/don-sim/src/systems/leader_production_setup_runtime.rs`.

## Exact retail fields

PDB class `LeaderDataEncrypt` is 248 bytes. The relevant rows are distinct:

| offset | field | count | retail XOR |
|---:|---|---:|---:|
| `+0x30` | `resource_cap` | 7 | `0x1281` |
| `+0x94` | `income` | 6 | `0x90236` |
| `+0xAC` | `rate` | 6 | `0x73862` |

`LeaderData::base_rate[6]` is instead at `LeaderData +0x4B0`. The setup loop clears it while
writing encrypted `rate`; the two arrays cannot share a Sim owner. `rate` is an AI planning
cache and can remain stale for a human leader. The canonical owner preserves that retail
state and does not add a fabricated freshness bit.

## Whole PE child: `get_mod_resource_cap`

`LeaderData::get_mod_resource_cap` `0x006D65B0` is a complete 162-byte child. It returns zero
when `starting_resources == 8`. Otherwise it decodes `resource_cap[resource]`, then applies
difficulty scaling only when both shared game byte `+0x820` bit 2 and `LeaderData` flags bit 2
are clear:

- difficulty 0: binary32 multiply by `0.5`, `cvttss2si`;
- difficulty 1: binary32 multiply by `0.75`, `cvttss2si`;
- every other/bypass path: binary32 multiply by `1.0`, `cvttss2si`.

The last path is not an integer identity. Values above binary32's exact-integer range can
round, and invalid `cvttss2si` conversions produce `INT_MIN`. The executable retains both
effects.

## Bounded setup cohort

The owner-complete cohort starts at `production_ai_setup` label `0x006C8676`. For each of six
resources retail:

1. clears `base_rate[resource]`;
2. calls the complete `get_mod_resource_cap` child;
3. decodes `income[resource]` and takes the signed minimum with the modified cap;
4. divides by 16 toward zero and writes encrypted `rate[resource]`;
5. for an available resource, updates `worst_good`, `best_good`, shortage flag bit 0 and the
   shortage count.

The executable also retains the function-entry reset and the `starting_resources == 8` early
return because that branch deliberately leaves `rate` and `base_rate` stale while OR-ing bit 3
into all six economy flags.

This is not a claim for the full 1,807-byte setup. The earlier difficulty/bucket-adjustment
prelude, the second six-resource flag loop, its Lakota correction, and the unconditional
`Leader::market_speculation` `0x006C8110` tail remain outside the boundary. No market state,
resource grant, purchase, sale, or build completion is synthesized.

## Save/resume evidence

The proposed canonical Leader-row projection is exactly six decoded little-endian i32 values
(24 bytes) in resource order. Retail XOR exists only at a process-memory boundary. The focused
suite covers exact encrypted/decoded round trips, every child gate, binary32 precision and
invalid conversion, the normal and starting-resources-8 setup paths, and three deterministic
four-seat economy sequences. Each four-seat sequence saves and restores the decoded rate row,
then compares the next natural cohort receipt and complete owned state with uninterrupted
execution. The harness changes no stockpile and injects no resources or completion events.
