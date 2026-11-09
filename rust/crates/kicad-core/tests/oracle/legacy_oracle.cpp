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

// Differential-test oracle for rust/crates/kicad-core/src/legacy_layers.rs.
//
// The functions under test (remapLegacyLayerLSET of pcbnew/pcb_plot_params.cpp, leg_layer2new
// and leg_mask2new of pcbnew/pcb_io/kicad_legacy/pcb_io_kicad_legacy.cpp) live in pcbnew
// translation units that cannot be compiled on their own, so tests/oracle/build_oracle.sh cuts
// their source text out of the original files into the two .inc files included below.
// The dump is tests/golden/legacy_layers_dump.txt, checked by tests/test_legacy_layers.rs.
#include <cstdio>
#include <cstdint>
#include <map>
#include <string>

#include <wx/string.h>
#include <wx/debug.h>

#include <layer_ids.h>
#include <lset.h>
#include <base_set.h>

#include "legacy_snip_plot_params.inc"
#include "legacy_snip_kicad_legacy.inc"

struct LCG
{
    uint64_t s;
    explicit LCG( uint64_t seed ) : s( seed ) {}
    uint64_t next()
    {
        s = s * 6364136223846793005ULL + 1442695040888963407ULL;
        return s >> 33;
    }
};

int main()
{
    wxSetAssertHandler( nullptr );

    printf( "COUNT %d\n", (int) LEGACY_PCB_LAYER_ID_COUNT );

    // remapLegacyLayerLSET: single bits, then random sets
    for( int bit = 0; bit < LEGACY_PCB_LAYER_ID_COUNT; ++bit )
    {
        BASE_SET legacy( LEGACY_PCB_LAYER_ID_COUNT );
        legacy.set( bit );
        printf( "REMAP1 %d %s\n", bit, remapLegacyLayerLSET( legacy ).FmtHex().c_str() );
    }

    LCG rng( 59 );

    for( int k = 0; k < 24; ++k )
    {
        BASE_SET legacy( LEGACY_PCB_LAYER_ID_COUNT );

        for( int bit = 0; bit < LEGACY_PCB_LAYER_ID_COUNT; ++bit )
        {
            if( rng.next() % 4 < (uint64_t) ( k % 4 ) + 1 )
                legacy.set( bit );
        }

        LSET remapped = remapLegacyLayerLSET( legacy );
        printf( "REMAP %s %zu %s\n", legacy.FmtHex().c_str(), remapped.size(), remapped.FmtHex().c_str() );
    }

    for( const char* hex : { "00010fc_ffffffff", "0000000_00000001", "0000000_80000000", "fffffff_ffffffff",
                             "0001000_00000000", "00000a8_80000001" } )
    {
        BASE_SET legacy( LEGACY_PCB_LAYER_ID_COUNT );
        int      consumed = legacy.ParseHex( std::string( hex ) );
        printf( "REMAPHEX %s %d %s\n", hex, consumed, remapLegacyLayerLSET( legacy ).FmtHex().c_str() );
    }

    // leg_layer2new / is_leg_copperlayer_valid
    for( int cu : { 1, 2, 4, 6, 8, 16 } )
    {
        std::string row, valid;

        for( int layer = -2; layer <= 34; ++layer )
        {
            row += " " + std::to_string( (int) leg_layer2new( cu, layer ) );
            valid += is_leg_copperlayer_valid( cu, layer ) ? '1' : '0';
        }

        printf( "L2N %d%s valid=%s\n", cu, row.c_str(), valid.c_str() );
    }

    // leg_mask2new
    for( int cu : { 2, 4, 16 } )
    {
        for( unsigned mask : { 0u, 1u, 0x8000u, 0x8001u, 0xFFFFu, 0x00C0FFFFu, 0x00888000u, 0x00440001u,
                               0x10000000u, 0x1FFF0000u, 0x1FFFFFFFu, 0xFFFFFFFFu, 0x20000000u, 0x0000FFFEu,
                               0x00000006u, 0x00007FFEu } )
        {
            LSET set = leg_mask2new( cu, mask );
            printf( "M2N %d %08x %zu %s\n", cu, mask, set.size(), set.FmtHex().c_str() );
        }
    }

    return 0;
}
