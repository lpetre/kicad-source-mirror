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

/*
 * Differential-test oracle for kicad_core::string_utils::wx_string_matches: wxString::Matches
 * of libwx_baseu (the wxWidgets 3.2 build the reference KiCad links) on pseudo-random texts
 * and masks over a small alphabet with wildcards, non-ASCII code points and embedded NULs.
 *
 * Output: one "m|<first case>|<hex bits>" line per 64 cases (bit i set when case
 * first + i matched).  Replayed by tests/test_wx_matches_oracle.rs; built by
 * build_wx_matches_oracle.sh.
 */

#include <cstdint>
#include <cstdio>
#include <wx/string.h>

static const int CASES = 64 * 800;

struct RNG // splitmix64
{
    uint64_t s;
    uint64_t next()
    {
        s += 0x9E3779B97F4A7C15ull;
        uint64_t z = s;
        z = ( z ^ ( z >> 30 ) ) * 0xBF58476D1CE4E5B9ull;
        z = ( z ^ ( z >> 27 ) ) * 0x94D049BB133111EBull;
        return z ^ ( z >> 31 );
    }
    int64_t below( int64_t n ) { return (int64_t) ( next() % (uint64_t) n ); }
};

static const wchar_t ALPHABET[] = { L'a', L'b', L'c', L'.', L'*', L'?', 0xE9, 0x1F600, 0 };

// alphabet size used for masks / texts: texts rarely contain wildcards, both rarely NULs
static wxString randomString( RNG& rng, bool aMask )
{
    int64_t      len = rng.below( 4 ) == 0 ? rng.below( 16 ) : rng.below( 7 );
    std::wstring s;

    for( int64_t i = 0; i < len; i++ )
    {
        int64_t r = rng.below( 100 );
        wchar_t c;

        if( r < 2 )
            c = 0;
        else if( r < 6 )
            c = ALPHABET[6 + rng.below( 2 )];
        else if( aMask && r < 40 )
            c = ALPHABET[4 + rng.below( 2 )];
        else if( !aMask && r < 9 )
            c = ALPHABET[4 + rng.below( 2 )];
        else
            c = ALPHABET[rng.below( 4 )];

        s.push_back( c );
    }

    return wxString( s.data(), s.size() );
}

int main()
{
    RNG      rng{ 77031 };
    uint64_t bits = 0;

    for( int c = 0; c < CASES; c++ )
    {
        wxString text = randomString( rng, false );
        wxString mask = randomString( rng, true );

        if( text.Matches( mask ) )
            bits |= uint64_t( 1 ) << ( c % 64 );

        if( c % 64 == 63 )
        {
            printf( "m|%d|%016llx\n", c - 63, (unsigned long long) bits );
            bits = 0;
        }
    }

    return 0;
}
