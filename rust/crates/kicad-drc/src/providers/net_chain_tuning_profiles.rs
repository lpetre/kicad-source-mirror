// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright The KiCad Developers, see AUTHORS.txt for contributors.
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the
// Free Software Foundation, either version 3 of the License, or (at your
// option) any later version.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
// General Public License for more details.
//
// You should have received a copy of the GNU General Public License along
// with this program.  If not, see <http://www.gnu.org/licenses/>.

//! Port of `pcbnew/drc/drc_test_provider_net_chain_tuning_profiles.cpp` (package DRC-34).
//!
//! Net chain tuning profile checks. Ensures that all nets in a net chain have a consistent
//! tuning profile assigned. Errors generated:
//! - `DRCE_NET_CHAIN_TUNING_PROFILES`
//!
//! The C++ collects the profiles per chain in a
//! `std::unordered_map<wxString, std::unordered_set<wxString>>` and reports in that map's
//! iteration order. [`StdStringKeyedOrder`] reproduces the libstdc++ (g++ 13) node order of
//! such a map (hash `std::hash<std::wstring>` of the chain name, prime rehash policy), so the
//! report order is the reference one.

use std::collections::BTreeSet;
use std::sync::Arc;

use kicad_core::eda_units::EdaUnits;
use kicad_core::layer_ids::UNDEFINED_LAYER;
use kicad_geom::hash::std_hash_bytes;
use kicad_geom::math::vector2::Vector2I;
use kicad_pcb::BoardCtx;
use kicad_pcb::drc_api::DRCE_NET_CHAIN_TUNING_PROFILES;

use super::run_host::{RunHost, report_aux};
use crate::engine::{DrcEngine, DrcRunState, DrcTestProviderRun};
use crate::item;
use crate::test_provider::{DrcTestProviderBase, no_path};

/// Port of `DRC_TEST_PROVIDER_NET_CHAIN_TUNING_PROFILES`.
#[derive(Clone, Debug)]
pub struct DrcTestProviderNetChainTuningProfiles {
    /// The `DRC_TEST_PROVIDER` base.
    pub base: DrcTestProviderBase,
}

impl Default for DrcTestProviderNetChainTuningProfiles {
    fn default() -> Self {
        DrcTestProviderNetChainTuningProfiles {
            base: DrcTestProviderBase::new("net chain tuning profiles"),
        }
    }
}

impl DrcTestProviderNetChainTuningProfiles {
    /// Port of `DRC_TEST_PROVIDER_NET_CHAIN_TUNING_PROFILES::Run`: false when the run was
    /// cancelled.
    pub fn run(&self, _engine: &DrcEngine, run: &mut DrcRunState<'_>, ctx: &BoardCtx<'_>) -> bool {
        if run.is_error_limit_exceeded(DRCE_NET_CHAIN_TUNING_PROFILES) {
            report_aux(run, "Net chain tuning profiles ignored. Tests not run.");
            return true; // continue with other tests
        }

        let mut host = RunHost(run);

        if !self
            .base
            .report_phase(&mut host, "Checking net chain tuning profiles...")
        {
            return false; // DRC cancelled
        }

        // Map of net chain name : net chain nets tuning profiles names
        let mut net_chain_tuning_profile_map: StdStringKeyedOrder<BTreeSet<String>> =
            StdStringKeyedOrder::new();

        for net in ctx.board.nets() {
            let net_chain = net.net_chain();

            if net_chain.is_empty() {
                continue;
            }

            // (a net without a netclass dereferences null in C++; every net has one after the
            // netclass synchronisation of the board load)
            let tuning_profile = net
                .netclass()
                .map_or(String::new(), |nc| nc.tuning_profile().to_string());

            net_chain_tuning_profile_map
                .index_or_default(net_chain)
                .insert(tuning_profile);
        }

        for (net_chain, tuning_profiles) in net_chain_tuning_profile_map.iter() {
            if tuning_profiles.len() == 1 {
                continue;
            }

            if !host.is_error_limit_exceeded(DRCE_NET_CHAIN_TUNING_PROFILES) {
                let msg =
                    format!("Net chain {net_chain} nets have multiple tuning profiles assigned");

                let mut drc_item = item::create(DRCE_NET_CHAIN_TUNING_PROFILES)
                    .expect("DRCE_NET_CHAIN_TUNING_PROFILES item");
                drc_item.set_error_message(&msg);

                self.base.report_violation(
                    &mut host,
                    Arc::new(drc_item),
                    Vector2I::default(),
                    UNDEFINED_LAYER.0,
                    &no_path,
                );
            }
        }

        !host.is_cancelled()
    }
}

impl DrcTestProviderRun for DrcTestProviderNetChainTuningProfiles {
    fn name(&self) -> &str {
        self.base.name
    }

    fn run_tests(
        &mut self,
        engine: &DrcEngine,
        run: &mut DrcRunState<'_>,
        ctx: &BoardCtx<'_>,
        units: EdaUnits,
    ) -> bool {
        self.base.set_user_units(units);
        self.run(engine, run, ctx)
    }
}

/// libstdc++ `__prime_list` (`hashtable-aux.cc`, 64-bit `unsigned long`), its first entries.
/// A map with more keys than the last entry is iterated in key order instead (see
/// [`StdStringKeyedOrder`]).
const LIBSTDCXX_PRIMES: [u64; 60] = [
    2, 3, 5, 7, 11, 13, 17, 19, 23, 29, 31, 37, 41, 43, 47, 53, 59, 61, 67, 71, 73, 79, 83, 89, 97,
    103, 109, 113, 127, 137, 139, 149, 157, 167, 179, 193, 199, 211, 227, 241, 257, 277, 293, 313,
    337, 359, 383, 409, 439, 467, 503, 541, 577, 619, 661, 709, 761, 823, 887, 953,
];

/// The node order of a libstdc++ (g++ 13) `std::unordered_map<wxString, V>` filled by
/// `operator[]` only (no erase).
///
/// `std::hash<wxString>` is `std::hash<std::wstring>()( s.ToStdWstring() )` (wx 3.2), i.e.
/// `std::_Hash_bytes` over the UTF-32 code units (4 bytes each, little endian) with the seed
/// `0xc70f6907`; the hash is cached in the node. Bucket = hash % bucket count. A new node is
/// inserted at the beginning of its bucket, or at the front of the whole list when its bucket
/// is empty (`_M_insert_bucket_begin`); a rehash re-links every node in list order with the
/// same rule (`_M_rehash_aux`); the `_Prime_rehash_policy` (max load factor 1.0, first
/// allocation of 11 -> 13 buckets) decides when. (The model of
/// `kicad_pcb::padstack::CopperPropsMap`, with string keys.)
#[derive(Clone, Debug)]
pub struct StdStringKeyedOrder<V> {
    /// (key, cached hash, value) in iteration order.
    nodes: Vec<(String, u64, V)>,
    /// `_M_bucket_count`.
    bucket_count: u64,
    /// `_Prime_rehash_policy::_M_next_resize`.
    next_resize: u64,
    /// More keys than [`LIBSTDCXX_PRIMES`] covers: iterate in key order.
    overflow: bool,
}

impl<V: Default> StdStringKeyedOrder<V> {
    /// A default-constructed, empty map.
    pub fn new() -> Self {
        StdStringKeyedOrder {
            nodes: Vec::new(),
            bucket_count: 1,
            next_resize: 0,
            overflow: false,
        }
    }

    /// `std::hash<wxString>` of `key`.
    pub fn hash(key: &str) -> u64 {
        let bytes: Vec<u8> = key.chars().flat_map(|c| (c as u32).to_le_bytes()).collect();
        std_hash_bytes(&bytes, 0xc70f_6907)
    }

    /// `_Prime_rehash_policy::_M_next_bkt( n )`; `None` past the modelled primes.
    fn next_bkt(&mut self, n: u64) -> Option<u64> {
        const FAST_BKT: [u64; 14] = [2, 2, 2, 3, 5, 5, 7, 7, 11, 11, 11, 11, 13, 13];

        if n < FAST_BKT.len() as u64 {
            if n == 0 {
                return Some(1);
            }

            self.next_resize = FAST_BKT[n as usize];
            return Some(FAST_BKT[n as usize]);
        }

        // lower_bound( __prime_list + 6, __last_prime, n )
        let next = LIBSTDCXX_PRIMES[6..].iter().copied().find(|&p| p >= n)?;
        self.next_resize = next;
        Some(next)
    }

    /// `_Prime_rehash_policy::_M_need_rehash( n_bkt, n_elt, 1 )`: the new bucket count, or
    /// `Some( None )` past the modelled primes.
    fn need_rehash(&mut self) -> Option<Option<u64>> {
        let n_bkt = self.bucket_count;
        let n_elt = self.nodes.len() as u64;

        if n_elt + 1 > self.next_resize {
            // If _M_next_resize is 0 it means that we have nothing allocated so far and that we
            // start inserting elements. In this case we start with an initial bucket size of
            // 11.
            let min_bkts = (n_elt + 1).max(if self.next_resize != 0 { 0 } else { 11 });

            if min_bkts >= n_bkt {
                return Some(self.next_bkt((min_bkts + 1).max(n_bkt * 2)));
            }

            self.next_resize = n_bkt;
        }

        None
    }

    /// `_M_insert_bucket_begin`: before the first node of the bucket, else at the front.
    fn insert_bucket_begin(
        nodes: &mut Vec<(String, u64, V)>,
        bucket_count: u64,
        node: (String, u64, V),
    ) -> usize {
        let bkt = node.1 % bucket_count;
        let pos = nodes
            .iter()
            .position(|(_, h, _)| h % bucket_count == bkt)
            .unwrap_or(0);
        nodes.insert(pos, node);
        pos
    }

    /// `operator[]( key )`: the entry, inserted with a default value when missing.
    pub fn index_or_default(&mut self, key: &str) -> &mut V {
        if let Some(i) = self.nodes.iter().position(|(k, _, _)| k == key) {
            return &mut self.nodes[i].2;
        }

        let node = (key.to_string(), Self::hash(key), V::default());

        if self.overflow {
            self.nodes.push(node);
            return &mut self.nodes.last_mut().expect("node").2;
        }

        if let Some(rehash) = self.need_rehash() {
            match rehash {
                Some(new_count) => {
                    let old = std::mem::take(&mut self.nodes);

                    for n in old {
                        Self::insert_bucket_begin(&mut self.nodes, new_count, n);
                    }

                    self.bucket_count = new_count;
                }
                None => {
                    // Not modelled: fall back to key order.
                    self.overflow = true;
                    self.nodes.push(node);
                    return &mut self.nodes.last_mut().expect("node").2;
                }
            }
        }

        let i = Self::insert_bucket_begin(&mut self.nodes, self.bucket_count, node);
        &mut self.nodes[i].2
    }

    /// The entries in iteration order (key order past the modelled sizes).
    pub fn iter(&self) -> Vec<(&str, &V)> {
        let mut entries: Vec<(&str, &V)> =
            self.nodes.iter().map(|(k, _, v)| (k.as_str(), v)).collect();

        if self.overflow {
            entries.sort_by(|a, b| a.0.cmp(b.0));
        }

        entries
    }
}

impl<V: Default> Default for StdStringKeyedOrder<V> {
    fn default() -> Self {
        Self::new()
    }
}
