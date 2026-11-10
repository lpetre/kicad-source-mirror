/*
 * This program source code file is part of KiCad, a free EDA CAD application.
 *
 * Copyright The KiCad Developers, see AUTHORS.txt for contributors.
 *
 * This program is free software: you can redistribute it and/or modify it
 * under the terms of the GNU General Public License as published by the
 * Free Software Foundation, either version 3 of the License, or (at your
 * option) any later version.
 *
 * This program is distributed in the hope that it will be useful, but
 * WITHOUT ANY WARRANTY; without even the implied warranty of
 * MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the GNU
 * General Public License for more details.
 *
 * You should have received a copy of the GNU General Public License along
 * with this program.  If not, see <http://www.gnu.org/licenses/>.
 */

// pcb-dump: the opt-in "evalrules" section (board command, --sections evalrules). Resolves
// constraints with DRC_ENGINE::EvalRules on the engine the QA load path created (package
// DRC-04 of the Rust port; replayed by rust/crates/kicad-drc/tests/evalrules_oracle.rs, which
// must walk the board and form the item pairs exactly as below).
//
// Keys are "<category>:<item key>[:<item key>]"; fields "c<constraint type>@<layer>" (numeric
// DRC_CONSTRAINT_T and PCB_LAYER_ID) or "own@<layer>" (GetCachedOwnClearance); values
// "t=<m_Type>;v=<min>/<opt>/<max>;n=<GetName()>;mr=<min rule name>;im=<m_ImplicitMin>;
// zc=<m_ZoneConnection>;d=<m_DisallowFlags>;s=<GetSeverity()>" (all numbers decimal).
//
// Items: board tracks (category track / arc / via) in m_tracks order; the pads of every
// footprint (pad); board zones (zone); footprint zones (fpzone); board drawings that are
// PCB_SHAPE / PCB_TEXT / PCB_TEXTBOX (drawing). Pairs: see dumpPairs().

#include "pcb_dump.h"

#include <board.h>
#include <board_design_settings.h>
#include <footprint.h>
#include <pad.h>
#include <pcb_track.h>
#include <zone.h>
#include <drc/drc_engine.h>
#include <drc/drc_rule.h>


namespace
{

const int K_PAIRS = 3;

std::string valueText( const MINOPTMAX<int>& aValue )
{
    std::string r;
    r += aValue.HasMin() ? V( aValue.Min() ) : std::string( "none" );
    r += "/";
    r += aValue.HasOpt() ? V( aValue.Opt() ) : std::string( "none" );
    r += "/";
    r += aValue.HasMax() ? V( aValue.Max() ) : std::string( "none" );
    return r;
}


std::string constraintText( const DRC_CONSTRAINT& c )
{
    std::string r;
    r += "t=" + V( (int) c.m_Type );
    r += ";v=" + valueText( c.m_Value );
    r += ";n=" + V( c.GetName() );
    r += ";mr=" + ( c.GetMinRule() ? V( c.GetRuleName( c.GetMinRule() ) ) : std::string( "-" ) );
    r += ";im=" + V( (int) c.m_ImplicitMin );
    r += ";zc=" + V( (int) c.m_ZoneConnection );
    r += ";d=" + V( c.m_DisallowFlags );
    r += ";s=" + V( (int) c.GetSeverity() );
    return r;
}


std::string field( DRC_CONSTRAINT_T aType, PCB_LAYER_ID aLayer )
{
    return "c" + std::to_string( (int) aType ) + "@" + std::to_string( (int) aLayer );
}


/// The enabled copper layers of a layer set in LSET::Seq() order.
LSEQ copperSeq( BOARD* aBoard, const LSET& aSet )
{
    return LSET( aSet & aBoard->GetEnabledLayers() & LSET::AllCuMask() ).Seq();
}


const DRC_CONSTRAINT_T UNARY_TYPES[] = {
    TRACK_WIDTH_CONSTRAINT,         VIA_DIAMETER_CONSTRAINT,
    HOLE_SIZE_CONSTRAINT,           ANNULAR_WIDTH_CONSTRAINT,
    EDGE_CLEARANCE_CONSTRAINT,      HOLE_CLEARANCE_CONSTRAINT,
    PHYSICAL_CLEARANCE_CONSTRAINT,  SILK_CLEARANCE_CONSTRAINT,
    TEXT_HEIGHT_CONSTRAINT,         SOLDER_MASK_EXPANSION_CONSTRAINT,
    SOLDER_PASTE_ABS_MARGIN_CONSTRAINT, SOLDER_PASTE_REL_MARGIN_CONSTRAINT,
    ZONE_CONNECTION_CONSTRAINT,     THERMAL_RELIEF_GAP_CONSTRAINT,
    THERMAL_SPOKE_WIDTH_CONSTRAINT
};


void dumpUnary( BOARD* aBoard, DRC_ENGINE* aEngine, const std::string& aCategory,
                BOARD_ITEM* aItem )
{
    {
        SCOPE s( "evalrules", aCategory + ":" + ItemKey( aItem ) );

        for( PCB_LAYER_ID layer : copperSeq( aBoard, aItem->GetLayerSet() ) )
        {
            EMIT( field( CLEARANCE_CONSTRAINT, layer ),
                  constraintText( aEngine->EvalRules( CLEARANCE_CONSTRAINT, aItem, nullptr,
                                                      layer ) ) );
            EMIT( "own@" + std::to_string( (int) layer ),
                  V( aEngine->GetCachedOwnClearance( aItem, layer ) ) );
        }

        PCB_LAYER_ID layer = aItem->GetLayer();

        for( DRC_CONSTRAINT_T type : UNARY_TYPES )
        {
            EMIT( field( type, layer ),
                  constraintText( aEngine->EvalRules( type, aItem, nullptr, layer ) ) );
        }
    }

    {
        // Separate category: the keepout conditions (intersectsKeepout) are another package
        // of the port.
        SCOPE s( "evalrules", "disallow:" + ItemKey( aItem ) );
        EMIT( field( DISALLOW_CONSTRAINT, UNDEFINED_LAYER ),
              constraintText( aEngine->EvalRules( DISALLOW_CONSTRAINT, aItem, nullptr,
                                                  UNDEFINED_LAYER ) ) );
    }
}


/// Binary constraints of one pair: CLEARANCE per shared enabled copper layer, then
/// HOLE_CLEARANCE, HOLE_TO_HOLE and DIFF_PAIR_GAP on UNDEFINED_LAYER; with aZoneConnection
/// also EvalZoneConnection, THERMAL_RELIEF_GAP and THERMAL_SPOKE_WIDTH per shared layer.
void dumpPair( BOARD* aBoard, DRC_ENGINE* aEngine, const std::string& aCategory,
               BOARD_ITEM* a, BOARD_ITEM* b, bool aZoneConnection )
{
    SCOPE s( "evalrules", aCategory + ":" + ItemKey( a ) + ":" + ItemKey( b ) );

    LSEQ shared = copperSeq( aBoard, a->GetLayerSet() & b->GetLayerSet() );

    for( PCB_LAYER_ID layer : shared )
    {
        EMIT( field( CLEARANCE_CONSTRAINT, layer ),
              constraintText( aEngine->EvalRules( CLEARANCE_CONSTRAINT, a, b, layer ) ) );

        if( aZoneConnection )
        {
            EMIT( "zc@" + std::to_string( (int) layer ),
                  constraintText( aEngine->EvalZoneConnection( a, b, layer ) ) );
            EMIT( field( THERMAL_RELIEF_GAP_CONSTRAINT, layer ),
                  constraintText( aEngine->EvalRules( THERMAL_RELIEF_GAP_CONSTRAINT, a, b,
                                                      layer ) ) );
            EMIT( field( THERMAL_SPOKE_WIDTH_CONSTRAINT, layer ),
                  constraintText( aEngine->EvalRules( THERMAL_SPOKE_WIDTH_CONSTRAINT, a, b,
                                                      layer ) ) );
        }
    }

    for( DRC_CONSTRAINT_T type :
         { HOLE_CLEARANCE_CONSTRAINT, HOLE_TO_HOLE_CONSTRAINT, DIFF_PAIR_GAP_CONSTRAINT } )
    {
        EMIT( field( type, UNDEFINED_LAYER ),
              constraintText( aEngine->EvalRules( type, a, b, UNDEFINED_LAYER ) ) );
    }
}


int netOf( BOARD_ITEM* aItem )
{
    return static_cast<BOARD_CONNECTED_ITEM*>( aItem )->GetNetCode();
}


/// Pairs:
///  pair_tt  each track i with the next K_PAIRS tracks j > i of another net;
///  pair_pp  the same within the pads;
///  pair_tp  track i with the pads P[( 7 * i + m ) % |P|], m = 0, 1, ... (at most
///           3 * K_PAIRS candidates), the first K_PAIRS of another net;
///  pair_zt  each board zone with the first K_PAIRS tracks of another net;
///  pair_zp  each board zone with the first K_PAIRS pads (any net), with the zone-connection
///           constraints (pad as a, zone as b);
///  pair_zz  each board zone with the next board zone.
void dumpPairs( BOARD* aBoard, DRC_ENGINE* aEngine, const std::vector<BOARD_ITEM*>& aTracks,
                const std::vector<BOARD_ITEM*>& aPads, const std::vector<BOARD_ITEM*>& aZones )
{
    auto nextOfOtherNets =
            [&]( const std::string& aCategory, const std::vector<BOARD_ITEM*>& aList )
            {
                for( size_t i = 0; i < aList.size(); ++i )
                {
                    int found = 0;

                    for( size_t j = i + 1; j < aList.size() && found < K_PAIRS; ++j )
                    {
                        if( netOf( aList[j] ) == netOf( aList[i] ) )
                            continue;

                        dumpPair( aBoard, aEngine, aCategory, aList[i], aList[j], false );
                        found++;
                    }
                }
            };

    nextOfOtherNets( "pair_tt", aTracks );
    nextOfOtherNets( "pair_pp", aPads );

    if( !aPads.empty() )
    {
        for( size_t i = 0; i < aTracks.size(); ++i )
        {
            int found = 0;

            for( size_t m = 0; m < 3 * K_PAIRS && found < K_PAIRS; ++m )
            {
                BOARD_ITEM* pad = aPads[( 7 * i + m ) % aPads.size()];

                if( netOf( pad ) == netOf( aTracks[i] ) )
                    continue;

                dumpPair( aBoard, aEngine, "pair_tp", aTracks[i], pad, false );
                found++;
            }
        }
    }

    for( size_t z = 0; z < aZones.size(); ++z )
    {
        int found = 0;

        for( size_t j = 0; j < aTracks.size() && found < K_PAIRS; ++j )
        {
            if( netOf( aTracks[j] ) == netOf( aZones[z] ) )
                continue;

            dumpPair( aBoard, aEngine, "pair_zt", aZones[z], aTracks[j], false );
            found++;
        }

        for( size_t j = 0; j < aPads.size() && (int) j < K_PAIRS; ++j )
            dumpPair( aBoard, aEngine, "pair_zp", aPads[j], aZones[z], true );

        if( z + 1 < aZones.size() )
            dumpPair( aBoard, aEngine, "pair_zz", aZones[z], aZones[z + 1], false );
    }
}

} // namespace


void DumpEvalRulesSection( BOARD* aBoard )
{
    std::shared_ptr<DRC_ENGINE> engine = aBoard->GetDesignSettings().m_DRCEngine;

    if( !engine )
    {
        g_out.Error( "evalrules", "engine", "no DRC engine" );
        return;
    }

    std::vector<BOARD_ITEM*> tracks, pads, zones;

    for( PCB_TRACK* track : aBoard->Tracks() )
    {
        tracks.push_back( track );
        dumpUnary( aBoard, engine.get(),
                   track->Type() == PCB_VIA_T   ? "via"
                   : track->Type() == PCB_ARC_T ? "arc"
                                                : "track",
                   track );
    }

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( PAD* pad : fp->Pads() )
        {
            pads.push_back( pad );
            dumpUnary( aBoard, engine.get(), "pad", pad );
        }
    }

    for( ZONE* zone : aBoard->Zones() )
    {
        zones.push_back( zone );
        dumpUnary( aBoard, engine.get(), "zone", zone );
    }

    for( FOOTPRINT* fp : aBoard->Footprints() )
    {
        for( ZONE* zone : fp->Zones() )
            dumpUnary( aBoard, engine.get(), "fpzone", zone );
    }

    for( BOARD_ITEM* item : aBoard->Drawings() )
    {
        if( item->Type() == PCB_SHAPE_T || item->Type() == PCB_TEXT_T
            || item->Type() == PCB_TEXTBOX_T )
        {
            dumpUnary( aBoard, engine.get(), "drawing", item );
        }
    }

    dumpPairs( aBoard, engine.get(), tracks, pads, zones );
}
