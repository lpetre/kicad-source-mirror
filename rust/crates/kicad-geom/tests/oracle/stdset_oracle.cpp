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
 * Differential-test oracle for kicad_geom::stdsort::StdSet / std_set_order: the iteration
 * order of libstdc++ std::set built from random sequences, with comparators that are (and
 * deliberately are not) strict weak orderings.  Only the C++ standard library is involved.
 *
 * Output: "set|<case>|<values...>" for the first VERBOSE_CASES cases, then one
 * "hash|<first case>|<fnv1a64>" line per BATCH cases (hash over every case's size and values).
 * Replayed by tests/oracle_stdset.rs; built by build_stdset_oracle.sh.
 */

#include <cstdint>
#include <cstdio>
#include <set>
#include <vector>

static const int CASES = 6000;
static const int BATCH = 100;
static const int VERBOSE_CASES = 40;

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

struct FNV
{
    uint64_t h = 0xcbf29ce484222325ull;
    void add( int64_t v )
    {
        for( int i = 0; i < 8; i++ )
        {
            h ^= (uint64_t) ( v >> ( 8 * i ) ) & 0xff;
            h *= 0x100000001b3ull;
        }
    }
};

// Comparator kinds:
//  0: a < b (strict weak ordering, many ties on small key ranges)
//  1: a < b - eps  (equivalence not transitive, like EDA_SHAPE::Compare with EPSILON)
//  2: random tournament table (asymmetric, not transitive)
//  3: cyclic: ( b - a ) mod 3 == 1 (rock-paper-scissors)
//  4: a < b && b - a <= eps + 1 (only close pairs are ordered)
//  5: |a| < |b| - eps with signed values
// Every comparator is irreflexive and asymmetric: without that, libstdc++ itself reads past
// the tree (undefined behaviour), which no port can reproduce.
struct CMP
{
    int kind;
    int64_t eps;
    const std::vector<uint8_t>* table;
    int64_t tableSize;

    bool operator()( int64_t a, int64_t b ) const
    {
        switch( kind )
        {
        case 0: return a < b;
        case 1: return a < b - eps;
        case 2: return ( *table )[a * tableSize + b] != 0;
        case 3: return ( ( b - a ) % 3 + 3 ) % 3 == 1;
        case 4: return a < b && b - a <= eps + 1;
        default:
        {
            int64_t aa = a < 0 ? -a : a;
            int64_t bb = b < 0 ? -b : b;
            return aa < bb - eps;
        }
        }
    }
};

int main()
{
    RNG rng{ 20261110 };
    FNV batch;

    for( int c = 0; c < CASES; c++ )
    {
        int     kind = (int) rng.below( 6 );
        int64_t n = rng.below( 4 ) == 0 ? rng.below( 400 ) : rng.below( 40 );
        int64_t range = 1 + rng.below( kind == 2 ? 30 : 200 );
        int64_t eps = rng.below( 6 );
        bool    hinted = rng.below( 2 ) == 0;

        std::vector<uint8_t> table;

        if( kind == 2 )
        {
            table.assign( range * range, 0 );

            for( int64_t a = 0; a < range; a++ )
            {
                for( int64_t b = a + 1; b < range; b++ )
                {
                    int64_t r = rng.below( 3 );

                    if( r == 1 )
                        table[a * range + b] = 1;
                    else if( r == 2 )
                        table[b * range + a] = 1;
                }
            }
        }

        std::vector<int64_t> items;

        for( int64_t i = 0; i < n; i++ )
            items.push_back( kind == 5 ? rng.below( 2 * range ) - range : rng.below( range ) );

        CMP cmp{ kind, eps, &table, range };
        std::vector<int64_t> out;

        if( hinted )
        {
            std::set<int64_t, CMP> s( items.begin(), items.end(), cmp );
            out.assign( s.begin(), s.end() );
        }
        else
        {
            std::set<int64_t, CMP> s( cmp );

            for( int64_t v : items )
                s.insert( v );

            out.assign( s.begin(), s.end() );
        }

        if( c < VERBOSE_CASES )
        {
            printf( "set|%d|", c );

            for( size_t i = 0; i < out.size(); i++ )
                printf( i ? " %lld" : "%lld", (long long) out[i] );

            printf( "\n" );
        }

        batch.add( (int64_t) out.size() );

        for( int64_t v : out )
            batch.add( v );

        if( c % BATCH == BATCH - 1 )
        {
            printf( "hash|%d|%016llx\n", c + 1 - BATCH, (unsigned long long) batch.h );
            batch = FNV();
        }
    }

    return 0;
}
