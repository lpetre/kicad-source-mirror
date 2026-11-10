// This program source code file is part of KiCad, a free EDA CAD application.
//
// Copyright (C) 2012 CERN.
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

//! Port of the writer part of `pcbnew/pcb_io/kicad_sexpr/pcb_io_kicad_sexpr.cpp` for package
//! PCBIO-11: the aggregated `(net_chains ..)` section of `format( const BOARD* )` (W:962-1038).
//! The legacy net classes are never written; `formatVariants` (W:828-850) and the consolidated
//! `(embedded_files ..)` section (W:1040-1063, `EMBEDDED_FILES::WriteEmbeddedFiles`) are
//! written by `board.rs`.

use std::cmp::Ordering;

use kicad_core::string_utils::value_string_compare;
use kicad_sexpr::IoError;

use super::PcbWriter;
use crate::board::Board;
use crate::ids::PadId;
use crate::netinfo::NetinfoItem;
use crate::traits::BoardItem;

/// `CHAIN_INFO`: the nets of one chain and its terminal pads.
struct ChainInfo<'b> {
    nets: Vec<&'b NetinfoItem>,
    pads: [Option<PadId>; 2],
}

impl PcbWriter<'_> {
    /// Port of the net-chain part of `format( const BOARD* aBoard )`: after writing all items,
    /// write the aggregated net chains section (if any): nets aggregated by chain name
    /// (`ValueStringCompare` order), written as `(net_chains (net_chain (name ..)
    /// (members (net ..) ..) [(terminal_pad ..)]) ..)`.
    pub(crate) fn format_net_chains(&mut self, board: &Board) -> Result<(), IoError> {
        // std::map<wxString, CHAIN_INFO, decltype( cmp )>: simple lexicographic ordering
        // using ValueStringCompare comparator logic. Kept sorted; keys the comparator holds
        // equivalent share one entry (the key first inserted), as in the std::map.
        let mut chains: Vec<(String, ChainInfo<'_>)> = Vec::new();

        let cmp = |a: &str, b: &str| -> Ordering {
            if value_string_compare(a, b) < 0 {
                Ordering::Less
            } else if value_string_compare(b, a) < 0 {
                Ordering::Greater
            } else {
                Ordering::Equal
            }
        };

        for net in board.nets() {
            if net.net_chain().is_empty()
                && net.terminal_pad(0).is_none()
                && net.terminal_pad(1).is_none()
            {
                continue; // nothing to aggregate
            }

            let mut chain_name = net.net_chain().to_string();

            if chain_name.is_empty()
                && (net.terminal_pad(0).is_some() || net.terminal_pad(1).is_some())
            {
                chain_name = net.netname().to_string(); // synthetic name for unnamed terminal association
            }

            // chains[chainName]
            let index = match chains.binary_search_by(|(key, _)| cmp(key, &chain_name)) {
                Ok(index) => index,
                Err(index) => {
                    chains.insert(
                        index,
                        (
                            chain_name,
                            ChainInfo {
                                nets: Vec::new(),
                                pads: [None, None],
                            },
                        ),
                    );
                    index
                }
            };

            let info = &mut chains[index].1;
            info.nets.push(net);

            for i in 0..2 {
                if net.terminal_pad(i).is_some() && info.pads[i].is_none() {
                    info.pads[i] = net.terminal_pad(i);
                }
            }
        }

        let mut count = 0usize;

        for (chain_name, si) in &chains {
            // Persist if: multi-net OR terminal pads OR explicit (non-empty) chain name
            if si.nets.len() > 1 || si.pads[0].is_some() || si.pads[1].is_some() || !chain_name.is_empty()
            {
                count += 1;
            }
        }

        if count != 0 {
            self.out.print(format_args!("(net_chains"))?;

            for (name, si) in &chains {
                if si.nets.len() == 1 && si.pads[0].is_none() && si.pads[1].is_none() && name.is_empty()
                {
                    continue;
                }

                let quoted = self.out.quotew(name);
                self.out.print(format_args!(" (net_chain (name {quoted})"))?;
                self.out.print(format_args!(" (members"))?;

                for n in &si.nets {
                    let quoted = self.out.quotew(n.netname());
                    self.out.print(format_args!(" (net {quoted})"))?;
                }

                self.out.print(format_args!(")"))?;

                for pad in si.pads.iter().flatten() {
                    // A dangling handle cannot occur: the pointers are resolved board pads.
                    let Some(pad) = board.pad(*pad) else {
                        continue;
                    };

                    let quoted = self.out.quotew(&BoardItem::uuid(pad).as_string());
                    self.out.print(format_args!(" (terminal_pad {quoted})"))?;
                }

                self.out.print(format_args!(")"))?;
            }

            self.out.print(format_args!(")"))?;
        }

        Ok(())
    }
}
