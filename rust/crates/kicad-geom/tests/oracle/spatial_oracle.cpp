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
 * Differential-test oracle for the spatial part of kicad-geom: libstdc++ std::sort and
 * friends, KIRTREE::PACKED_RTREE / DYNAMIC_RTREE (libs/kimath/include/geometry/rtree),
 * the bundled delaunator-cpp and nanoflann.
 *
 * It generates the cases with a small deterministic generator (splitmix64) and prints, per
 * section, the number of cases and an FNV-1a hash over the results of all cases (exact
 * permutations, visit orders, triangulations, double bit patterns).  The Rust mirror is
 * tests/oracle/spatial_sections.rs, the replay test tests/oracle_spatial.rs; the output of
 * this program is committed as tests/golden/spatial_oracle.txt.
 *
 * Usage: spatial_oracle [--dump FILE]   (FILE gets one "section index hash" line per case)
 *
 * Keep it in lock-step with spatial_sections.rs: every random number is drawn in the same
 * order (no two RNG calls in one expression or argument list).
 */

#include <geometry/rtree/dynamic_rtree.h>
#include <geometry/rtree/packed_rtree.h>

#include <delaunator.hpp>
#include <nanoflann.hpp>

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <cstdio>
#include <cstring>
#include <queue>
#include <stdexcept>
#include <string>
#include <vector>


struct RNG
{
    uint64_t s;

    explicit RNG( uint64_t aSeed ) : s( aSeed ) {}

    uint64_t next()
    {
        s += 0x9E3779B97F4A7C15ULL;
        uint64_t z = s;
        z = ( z ^ ( z >> 30 ) ) * 0xBF58476D1CE4E5B9ULL;
        z = ( z ^ ( z >> 27 ) ) * 0x94D049BB133111EBULL;
        return z ^ ( z >> 31 );
    }

    // inclusive range
    int64_t range( int64_t lo, int64_t hi )
    {
        return lo + (int64_t) ( next() % (uint64_t) ( hi - lo + 1 ) );
    }
};


struct HASHER
{
    uint64_t h = 0xcbf29ce484222325ULL;

    void u( uint64_t v )
    {
        for( int i = 0; i < 8; ++i )
        {
            h ^= ( v >> ( 8 * i ) ) & 0xff;
            h *= 0x100000001b3ULL;
        }
    }

    void i( int64_t v ) { u( (uint64_t) v ); }

    void d( double x )
    {
        uint64_t b;
        memcpy( &b, &x, 8 );
        u( b );
    }
};


static FILE* g_dump = nullptr;


struct SECTION
{
    std::string name;
    uint64_t    count = 0;
    HASHER      h;

    explicit SECTION( const char* aName ) : name( aName ) {}

    void add( const HASHER& aCase )
    {
        if( g_dump )
            fprintf( g_dump, "%s %llu %016llx\n", name.c_str(), (unsigned long long) count,
                     (unsigned long long) aCase.h );

        count++;
        h.u( aCase.h );
    }

    void print() const
    {
        printf( "%s %llu %016llx\n", name.c_str(), (unsigned long long) count,
                (unsigned long long) h.h );
    }
};


// ---------------------------------------------------------------------------------------
// std::sort, std::partial_sort, std::nth_element, heaps
// ---------------------------------------------------------------------------------------

struct TAGGED
{
    int64_t key;
    int64_t tag;
};


// Arrays of (key, tag) with many ties: pattern p, length n
static std::vector<TAGGED> genSortArray( RNG& rng, int pattern, int64_t n )
{
    std::vector<TAGGED> v;
    int64_t             keys = 1;

    switch( pattern )
    {
    case 0: keys = 1; break;
    case 1: keys = 2; break;
    case 2: keys = 3 + rng.range( 0, 10 ); break;
    case 3: keys = 1 + n / 4; break;
    case 4: keys = 1 + n; break;
    default: keys = 1000000000; break;
    }

    for( int64_t i = 0; i < n; ++i )
    {
        int64_t key = 0;

        switch( pattern )
        {
        case 6: key = i / 3; break;                         // sorted with ties
        case 7: key = ( n - i ) / 2; break;                 // reversed with ties
        case 8: key = std::min( i, n - i ); break;          // organ pipe
        case 9: key = i % 17; break;                        // sawtooth
        case 10: key = ( i % 2 ) ? i : n - i; break;        // interleaved
        default: key = (int64_t) ( rng.next() % (uint64_t) keys ); break;
        }

        v.push_back( { key, i } );
    }

    return v;
}


static void hashTags( HASHER& h, const std::vector<TAGGED>& v )
{
    h.u( v.size() );

    for( const TAGGED& t : v )
    {
        h.i( t.key );
        h.i( t.tag );
    }
}


static int64_t sortLength( RNG& rng, int64_t aCase )
{
    static const int64_t special[] = { 0, 1, 2, 3, 15, 16, 17, 18, 31, 32, 33, 34, 64, 65 };

    if( aCase < 14 * 4 )
        return special[aCase % 14];

    int64_t sel = rng.range( 0, 9 );

    if( sel < 5 )
        return rng.range( 0, 100 );
    else if( sel < 9 )
        return rng.range( 0, 2000 );
    else
        return rng.range( 2000, 6000 );
}


static void sectionSort()
{
    SECTION sec( "std_sort" );
    RNG     rng( 101 );

    auto less = []( const TAGGED& a, const TAGGED& b ) { return a.key < b.key; };

    for( int64_t c = 0; c < 3000; ++c )
    {
        int64_t n = sortLength( rng, c );
        int     pattern = (int) ( c % 11 );
        std::vector<TAGGED> v = genSortArray( rng, pattern, n );

        std::sort( v.begin(), v.end(), less );

        HASHER h;
        hashTags( h, v );
        sec.add( h );
    }

    // McIlroy's antiquicksort adversary drives the median-of-three quicksort into its
    // quadratic case, so that introsort takes the heap sort fallback
    for( int64_t c = 0; c < 40; ++c )
    {
        int64_t n = 100 + c * 250;
        int64_t gas = n;
        std::vector<int64_t> val( n, gas );
        int64_t nsolid = 0;
        int64_t candidate = 0;
        std::vector<int64_t> ids( n );

        for( int64_t i = 0; i < n; ++i )
            ids[i] = i;

        std::sort( ids.begin(), ids.end(),
                   [&]( int64_t x, int64_t y )
                   {
                       if( val[x] == gas && val[y] == gas )
                       {
                           if( x == candidate )
                               val[x] = nsolid++;
                           else
                               val[y] = nsolid++;
                       }

                       if( val[x] == gas )
                           candidate = x;
                       else if( val[y] == gas )
                           candidate = y;

                       return val[x] < val[y];
                   } );

        // now sort the frozen adversarial values with ties on key / 2
        std::vector<TAGGED> v;

        for( int64_t i = 0; i < n; ++i )
            v.push_back( { val[i] / 2, i } );

        std::sort( v.begin(), v.end(), less );

        HASHER h;
        h.u( n );

        for( int64_t id : ids )
            h.i( id );

        hashTags( h, v );
        sec.add( h );
    }

    sec.print();
}


static void sectionPartialSort()
{
    SECTION sec( "std_partial_sort_nth_element" );
    RNG     rng( 202 );

    auto less = []( const TAGGED& a, const TAGGED& b ) { return a.key < b.key; };

    for( int64_t c = 0; c < 1500; ++c )
    {
        int64_t n = sortLength( rng, c );
        int     pattern = (int) ( c % 11 );
        std::vector<TAGGED> v = genSortArray( rng, pattern, n );
        std::vector<TAGGED> w = v;
        int64_t middle = rng.range( 0, n );
        int64_t nth = rng.range( 0, n );

        std::partial_sort( v.begin(), v.begin() + middle, v.end(), less );

        if( nth < n )
            std::nth_element( w.begin(), w.begin() + nth, w.end(), less );

        HASHER h;
        h.i( middle );
        h.i( nth );
        hashTags( h, v );
        hashTags( h, w );
        sec.add( h );
    }

    sec.print();
}


static void sectionHeap()
{
    SECTION sec( "std_heap_priority_queue" );
    RNG     rng( 303 );

    auto less = []( const TAGGED& a, const TAGGED& b ) { return a.key < b.key; };
    auto greater = []( const TAGGED& a, const TAGGED& b ) { return a.key > b.key; };

    for( int64_t c = 0; c < 1000; ++c )
    {
        int64_t n = rng.range( 0, 300 );
        int     pattern = (int) ( c % 11 );
        std::vector<TAGGED> v = genSortArray( rng, pattern, n );

        HASHER h;

        std::vector<TAGGED> heap = v;
        std::make_heap( heap.begin(), heap.end(), less );
        hashTags( h, heap );

        std::sort_heap( heap.begin(), heap.end(), less );
        hashTags( h, heap );

        // a random script of push / pop on a min-queue
        std::priority_queue<TAGGED, std::vector<TAGGED>, decltype( greater )> pq( greater );
        int64_t ops = rng.range( 0, 600 );
        int64_t next = 0;

        for( int64_t k = 0; k < ops; ++k )
        {
            int64_t sel = rng.range( 0, 2 );

            if( sel < 2 && next < n )
            {
                pq.push( v[next] );
                next++;
            }
            else if( !pq.empty() )
            {
                h.i( pq.top().key );
                h.i( pq.top().tag );
                pq.pop();
            }
        }

        while( !pq.empty() )
        {
            h.i( pq.top().tag );
            pq.pop();
        }

        sec.add( h );
    }

    sec.print();
}


// ---------------------------------------------------------------------------------------
// r-trees
// ---------------------------------------------------------------------------------------

template <int DIM>
struct BOX
{
    int min[DIM];
    int max[DIM];
};


// A random box.  DIM == 3 is { layer, x, y } like CN_RTREE.
template <int DIM>
static BOX<DIM> genBox( RNG& rng, int aKind, const std::vector<BOX<DIM>>& aSoFar )
{
    BOX<DIM> b;
    int      first = 0;

    if( DIM == 3 )
    {
        int64_t l0 = rng.range( 0, 31 );
        int64_t sel = rng.range( 0, 3 );
        int64_t l1 = sel == 0 ? rng.range( l0, 31 ) : l0;
        b.min[0] = (int) l0;
        b.max[0] = (int) l1;
        first = 1;
    }

    for( int d = first; d < DIM; ++d )
    {
        int64_t lo = 0;
        int64_t size = 0;

        switch( aKind )
        {
        case 0: // board-like
            lo = rng.range( -150000000, 150000000 );
            size = rng.range( 0, 5000000 );
            break;

        case 1: // clustered around a few centres: many equal centres
        {
            int64_t centre = rng.range( 0, 4 ) * 25000000;
            int64_t half = rng.range( 0, 3 ) * 100000;
            lo = centre - half;
            size = 2 * half;
            break;
        }

        case 2: // pad grid, 1.27 mm pitch
        {
            int64_t cell = rng.range( 0, 40 );
            int64_t half = rng.range( 1, 2 ) * 300000;
            lo = cell * 1270000 - half;
            size = 2 * half;
            break;
        }

        case 3: // points
            lo = rng.range( -1000, 1000 );
            size = 0;
            break;

        case 4: // full int range
            lo = rng.range( INT32_MIN, INT32_MAX );
            size = rng.range( 0, (int64_t) INT32_MAX - lo );
            break;

        default: // small integers: heavy overlap and duplicates
            lo = rng.range( -20, 20 );
            size = rng.range( 0, 10 );
            break;
        }

        b.min[d] = (int) lo;
        b.max[d] = (int) ( lo + size );
    }

    // sometimes an exact copy of an earlier box
    int64_t dup = rng.range( 0, 9 );

    if( dup == 0 && !aSoFar.empty() )
    {
        int64_t idx = rng.range( 0, (int64_t) aSoFar.size() - 1 );
        b = aSoFar[idx];
    }

    return b;
}


template <int DIM>
static BOX<DIM> genQuery( RNG& rng, const std::vector<BOX<DIM>>& aItems )
{
    BOX<DIM> q;
    int64_t  sel = rng.range( 0, 4 );

    if( sel == 0 && !aItems.empty() )
    {
        // an item's box, inflated
        int64_t idx = rng.range( 0, (int64_t) aItems.size() - 1 );
        int64_t grow = rng.range( 0, 1000000 );
        q = aItems[idx];

        for( int d = 0; d < DIM; ++d )
        {
            q.min[d] = (int) std::max<int64_t>( INT32_MIN, (int64_t) q.min[d] - grow );
            q.max[d] = (int) std::min<int64_t>( INT32_MAX, (int64_t) q.max[d] + grow );
        }
    }
    else if( sel == 1 )
    {
        // everything
        for( int d = 0; d < DIM; ++d )
        {
            q.min[d] = INT32_MIN;
            q.max[d] = INT32_MAX;
        }
    }
    else
    {
        for( int d = 0; d < DIM; ++d )
        {
            int64_t a = sel == 2 ? rng.range( -20, 20 ) : rng.range( -160000000, 160000000 );
            int64_t s = sel == 4 ? rng.range( 0, 100000000 ) : rng.range( 0, 30 );
            q.min[d] = (int) a;
            q.max[d] = (int) std::min<int64_t>( INT32_MAX, a + s );
        }
    }

    return q;
}


template <int DIM>
static std::vector<BOX<DIM>> genBoxes( RNG& rng, int64_t aCase )
{
    static const int64_t special[] = { 0, 1, 2, 15, 16, 17, 31, 32, 33, 255, 256, 257 };
    int64_t              n;

    if( aCase < 12 )
        n = special[aCase];
    else
    {
        int64_t sel = rng.range( 0, 3 );
        n = sel == 0 ? rng.range( 0, 40 ) : rng.range( 0, 1500 );
    }

    int64_t mixed = rng.range( 0, 1 );
    int     kind = (int) ( aCase % 6 );

    std::vector<BOX<DIM>> items;

    for( int64_t i = 0; i < n; ++i )
    {
        int k = kind;

        if( mixed )
            k = (int) rng.range( 0, 5 );

        items.push_back( genBox<DIM>( rng, k, items ) );
    }

    return items;
}


// Runs the queries against a tree with a Search( min, max, visitor ) method; records visit
// order, and an early-terminated search
template <int DIM, class TREE>
static void hashQueries( HASHER& h, RNG& rng, const TREE& aTree,
                         const std::vector<BOX<DIM>>& aItems, int64_t aCount )
{
    for( int64_t k = 0; k < aCount; ++k )
    {
        BOX<DIM> q = genQuery<DIM>( rng, aItems );
        int64_t  limit = rng.range( 0, 3 ) == 0 ? rng.range( 1, 20 ) : -1;
        int64_t  seen = 0;

        auto visitor = [&]( const int& aId ) -> bool
        {
            h.i( aId );
            seen++;
            return limit < 0 || seen < limit;
        };

        int found = aTree.Search( q.min, q.max, visitor );
        h.i( found );
    }
}


template <int DIM>
static void sectionPacked( const char* aName, uint64_t aSeed, int64_t aCases )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    for( int64_t c = 0; c < aCases; ++c )
    {
        std::vector<BOX<DIM>> items = genBoxes<DIM>( rng, c );

        typename KIRTREE::PACKED_RTREE<int, int, DIM, 16>::Builder builder;
        builder.Reserve( items.size() );

        for( size_t i = 0; i < items.size(); ++i )
            builder.Add( items[i].min, items[i].max, (int) i );

        KIRTREE::PACKED_RTREE<int, int, DIM, 16> tree = builder.Build();

        HASHER h;
        h.u( tree.size() );
        h.u( tree.empty() );

        for( int id : tree )
            h.i( id );

        hashQueries<DIM>( h, rng, tree, items, 25 );
        sec.add( h );
    }

    sec.print();
}


template <int DIM>
static void hashDynamicState( HASHER& h, RNG& rng, const KIRTREE::DYNAMIC_RTREE<int, int, DIM, 16>& aTree,
                              const std::vector<BOX<DIM>>& aItems )
{
    h.u( aTree.size() );

    for( auto it = aTree.begin(); it != aTree.end(); ++it )
        h.i( *it );

    hashQueries<DIM>( h, rng, aTree, aItems, 12 );

    // the lazy range
    BOX<DIM> q = genQuery<DIM>( rng, aItems );

    for( int id : aTree.Overlapping( q.min, q.max ) )
        h.i( id );

    // nearest neighbours
    for( int k = 0; k < 2; ++k )
    {
        int     pt[DIM];
        int64_t kk = rng.range( 0, 12 );

        for( int d = 0; d < DIM; ++d )
            pt[d] = (int) rng.range( -150000000, 150000000 );

        std::vector<std::pair<int64_t, int>> res;
        aTree.NearestNeighbors( pt, (int) kk, res );

        for( const auto& [dist, id] : res )
        {
            h.i( dist );
            h.i( id );
        }
    }
}


template <int DIM>
static void sectionDynamic( const char* aName, uint64_t aSeed, int64_t aCases )
{
    SECTION sec( aName );
    RNG     rng( aSeed );

    using TREE = KIRTREE::DYNAMIC_RTREE<int, int, DIM, 16>;

    for( int64_t c = 0; c < aCases; ++c )
    {
        std::vector<BOX<DIM>> items = genBoxes<DIM>( rng, c );
        HASHER                h;

        // 1. Insert one by one
        TREE tree;

        for( size_t i = 0; i < items.size(); ++i )
            tree.Insert( items[i].min, items[i].max, (int) i );

        hashDynamicState<DIM>( h, rng, tree, items );

        // 2. Remove: every odd id with its box, every 4th with a wrong box (fallback path),
        //    and one id that is not there
        for( size_t i = 0; i < items.size(); ++i )
        {
            if( i % 2 == 1 )
            {
                h.u( tree.Remove( items[i].min, items[i].max, (int) i ) );
            }
            else if( i % 4 == 0 )
            {
                BOX<DIM> wrong = genQuery<DIM>( rng, items );
                h.u( tree.Remove( wrong.min, wrong.max, (int) i ) );
            }
        }

        int zero[DIM] = {};
        h.u( tree.Remove( zero, zero, -1 ) );

        hashDynamicState<DIM>( h, rng, tree, items );

        // 3. Insert again on top of the remaining tree
        for( size_t i = 0; i < items.size(); i += 2 )
            tree.Insert( items[i].min, items[i].max, (int) ( i + 100000 ) );

        hashDynamicState<DIM>( h, rng, tree, items );

        // 4. Bulk load (replaces everything), then insert and remove on top
        std::vector<typename TREE::BULK_ENTRY> entries;

        for( size_t i = 0; i < items.size(); ++i )
        {
            typename TREE::BULK_ENTRY e;

            for( int d = 0; d < DIM; ++d )
            {
                e.min[d] = items[i].min[d];
                e.max[d] = items[i].max[d];
            }

            e.data = (int) i;
            entries.push_back( e );
        }

        tree.BulkLoad( entries );
        hashDynamicState<DIM>( h, rng, tree, items );

        for( size_t i = 0; i < items.size(); i += 3 )
            h.u( tree.Remove( items[i].min, items[i].max, (int) i ) );

        for( size_t i = 0; i < items.size(); i += 5 )
            tree.Insert( items[i].min, items[i].max, (int) ( i + 200000 ) );

        hashDynamicState<DIM>( h, rng, tree, items );

        tree.RemoveAll();
        h.u( tree.size() );
        h.u( tree.empty() );

        sec.add( h );
    }

    sec.print();
}


// ---------------------------------------------------------------------------------------
// delaunator
// ---------------------------------------------------------------------------------------

static std::vector<double> genPoints( RNG& rng, int64_t aCase )
{
    std::vector<double> c;
    int                 kind = (int) ( aCase % 9 );
    int64_t             n = rng.range( 3, 12 );
    int64_t             sel = rng.range( 0, 3 );

    if( sel == 1 )
        n = rng.range( 3, 300 );
    else if( sel == 2 )
        n = rng.range( 3, 1500 );

    auto push = [&]( int64_t x, int64_t y )
    {
        c.push_back( (double) x );
        c.push_back( (double) y );
    };

    switch( kind )
    {
    case 0: // uniform, large range
        for( int64_t i = 0; i < n; ++i )
        {
            int64_t x = rng.range( -150000000, 150000000 );
            int64_t y = rng.range( -150000000, 150000000 );
            push( x, y );
        }
        break;

    case 1: // tiny integer range: duplicates and collinear triples
        for( int64_t i = 0; i < n; ++i )
        {
            int64_t x = rng.range( 0, 6 );
            int64_t y = rng.range( 0, 6 );
            push( x, y );
        }
        break;

    case 2: // subset of a pad grid (symmetric: equal distances)
        for( int64_t i = 0; i < n; ++i )
        {
            int64_t x = rng.range( 0, 30 );
            int64_t y = rng.range( 0, 30 );
            push( x * 1270000, y * 1270000 );
        }
        break;

    case 3: // full grid
    {
        int64_t w = rng.range( 2, 40 );
        int64_t hgt = rng.range( 2, 40 );

        for( int64_t i = 0; i < w; ++i )
            for( int64_t j = 0; j < hgt; ++j )
                push( i * 2540000, j * 1270000 );

        break;
    }

    case 4: // grid with nm jitter
        for( int64_t i = 0; i < n; ++i )
        {
            int64_t x = rng.range( 0, 20 );
            int64_t y = rng.range( 0, 20 );
            int64_t jx = rng.range( -2, 2 );
            int64_t jy = rng.range( -2, 2 );
            push( x * 1000000 + jx, y * 1000000 + jy );
        }
        break;

    case 5: // circle rounded to nm (rational parametrisation: no libm involved)
    {
        double r = (double) rng.range( 1000, 50000000 );

        for( int64_t i = 0; i < n; ++i )
        {
            double t = 2.0 * (double) i / (double) n - 1.0;
            double x = r * ( 1.0 - t * t ) / ( 1.0 + t * t );
            double y = r * ( 2.0 * t ) / ( 1.0 + t * t );
            push( std::llround( x ), std::llround( y ) );
            push( std::llround( -x ), std::llround( -y ) );
        }

        break;
    }

    case 6: // near-collinear line
    {
        int64_t dy = rng.range( 0, 3 );

        for( int64_t i = 0; i < n; ++i )
        {
            int64_t x = rng.range( 0, 300000000 );
            int64_t y = rng.range( 0, dy );
            push( x, y );
        }

        break;
    }

    case 7: // two rows (QFP side / DIP)
        for( int64_t i = 0; i < n; ++i )
        {
            int64_t row = rng.range( 0, 1 );
            int64_t pos = rng.range( 0, 60 );
            push( pos * 500000, row * 7620000 );
        }
        break;

    default: // exactly collinear (error)
        for( int64_t i = 0; i < n; ++i )
        {
            int64_t t = rng.range( -1000, 1000 );
            push( t * 3, t * 7 );
        }

        break;
    }

    return c;
}


static void hashSizeVec( HASHER& h, const std::vector<size_t>& v )
{
    h.u( v.size() );

    for( size_t x : v )
        h.i( x == delaunator::INVALID_INDEX ? -1 : (int64_t) x );
}


static void sectionDelaunator()
{
    SECTION sec( "delaunator" );
    RNG     rng( 404 );

    for( int64_t c = 0; c < 2500; ++c )
    {
        std::vector<double> coords = genPoints( rng, c );
        HASHER              h;

        h.u( coords.size() );

        // All points identical: delaunator-cpp indexes m_points[INVALID_INDEX] (undefined
        // behaviour), so that input is not compared
        bool allSame = true;

        for( size_t i = 2; i < coords.size(); i += 2 )
        {
            if( coords[i] != coords[0] || coords[i + 1] != coords[1] )
                allSame = false;
        }

        if( allSame )
        {
            h.i( 3 );
            sec.add( h );
            continue;
        }

        try
        {
            delaunator::Delaunator d( coords );
            h.i( 0 );
            hashSizeVec( h, d.triangles );
            hashSizeVec( h, d.halfedges );
            hashSizeVec( h, d.hull_prev );
            hashSizeVec( h, d.hull_next );
            hashSizeVec( h, d.hull_tri );
            h.i( (int64_t) d.hull_start );
            h.d( d.get_hull_area() );
            h.d( d.get_triangle_area() );
        }
        catch( const std::runtime_error& e )
        {
            h.i( std::string( e.what() ) == "not triangulation" ? 1 : 2 );
        }

        sec.add( h );
    }

    sec.print();
}


// ---------------------------------------------------------------------------------------
// nanoflann
// ---------------------------------------------------------------------------------------

struct POINTS_ADAPTOR
{
    std::vector<std::pair<int, int>> pts;

    size_t kdtree_get_point_count() const { return pts.size(); }

    double kdtree_get_pt( const size_t idx, const size_t dim ) const
    {
        return dim == 0 ? static_cast<double>( pts[idx].first )
                        : static_cast<double>( pts[idx].second );
    }

    template <class BBOX>
    bool kdtree_get_bbox( BBOX& ) const
    {
        return false;
    }
};


using KDTREE = nanoflann::KDTreeSingleIndexAdaptor<nanoflann::L2_Simple_Adaptor<double, POINTS_ADAPTOR>,
                                                   POINTS_ADAPTOR, 2>;


// End points of outline-like shapes: chains with gaps, near misses, duplicates
static std::vector<std::pair<int, int>> genEndpoints( RNG& rng, int64_t aCase )
{
    std::vector<std::pair<int, int>> pts;
    int                              kind = (int) ( aCase % 4 );
    int64_t                          shapes = rng.range( 0, 3 ) == 0 ? rng.range( 0, 8 )
                                                                     : rng.range( 1, 400 );

    int64_t x = rng.range( -100000000, 100000000 );
    int64_t y = rng.range( -100000000, 100000000 );

    for( int64_t s = 0; s < shapes; ++s )
    {
        if( kind == 0 )
        {
            // a chain: end of one shape == start of the next, sometimes off by a few nm
            int64_t gap = rng.range( 0, 4 ) == 0 ? rng.range( -3, 3 ) : 0;
            int64_t nx = x + rng.range( -5000000, 5000000 );
            int64_t ny = y + rng.range( -5000000, 5000000 );
            pts.emplace_back( (int) ( x + gap ), (int) y );
            pts.emplace_back( (int) nx, (int) ny );
            x = nx;
            y = ny;
        }
        else if( kind == 1 )
        {
            // coarse grid: many exact duplicates and equal distances
            int64_t ax = rng.range( 0, 6 );
            int64_t ay = rng.range( 0, 6 );
            int64_t bx = rng.range( 0, 6 );
            int64_t by = rng.range( 0, 6 );
            pts.emplace_back( (int) ( ax * 1000 ), (int) ( ay * 1000 ) );
            pts.emplace_back( (int) ( bx * 1000 ), (int) ( by * 1000 ) );
        }
        else if( kind == 2 )
        {
            // all on one axis
            int64_t a = rng.range( -1000000, 1000000 );
            int64_t b = rng.range( -1000000, 1000000 );
            pts.emplace_back( (int) a, 0 );
            pts.emplace_back( (int) b, 0 );
        }
        else
        {
            int64_t ax = rng.range( -100000000, 100000000 );
            int64_t ay = rng.range( -100000000, 100000000 );
            int64_t bx = rng.range( -100000000, 100000000 );
            int64_t by = rng.range( -100000000, 100000000 );
            pts.emplace_back( (int) ax, (int) ay );
            pts.emplace_back( (int) bx, (int) by );
        }
    }

    return pts;
}


static void sectionNanoflann()
{
    SECTION sec( "nanoflann" );
    RNG     rng( 505 );

    for( int64_t c = 0; c < 1000; ++c )
    {
        POINTS_ADAPTOR adaptor;
        adaptor.pts = genEndpoints( rng, c );
        KDTREE kdTree( 2, adaptor );

        HASHER h;
        h.u( adaptor.pts.size() );

        for( int k = 0; k < 24; ++k )
        {
            double  query[2];
            int64_t sel = rng.range( 0, 2 );

            if( sel == 0 && !adaptor.pts.empty() )
            {
                int64_t idx = rng.range( 0, (int64_t) adaptor.pts.size() - 1 );
                int64_t dx = rng.range( -2, 2 );
                query[0] = (double) adaptor.pts[idx].first + (double) dx;
                query[1] = (double) adaptor.pts[idx].second;
            }
            else
            {
                int64_t qx = rng.range( -100000000, 100000000 );
                int64_t qy = rng.range( -100000000, 100000000 );
                query[0] = (double) qx;
                query[1] = (double) qy;
            }

            // radius: like findNeighbours, eps * eps; sometimes exactly an existing distance
            int64_t rsel = rng.range( 0, 3 );
            double  eps = rsel == 0 ? 1000.0 : rsel == 1 ? 2.0 : (double) rng.range( 0, 20000000 );
            double  radius_sq = eps * eps;

            std::vector<nanoflann::ResultItem<uint32_t, double>> matches;
            size_t nFound = kdTree.radiusSearch( query, radius_sq, matches );
            h.u( nFound );

            for( const auto& m : matches )
            {
                h.u( m.first );
                h.d( m.second );
            }

            nanoflann::SearchParameters unsorted( 0, false );
            kdTree.radiusSearch( query, radius_sq, matches, unsorted );

            for( const auto& m : matches )
                h.u( m.first );

            uint32_t indices[8] = { 0 };
            double   dists[8];
            const size_t found = kdTree.knnSearch( query, 8, indices, dists );
            h.u( found );

            for( size_t i = 0; i < found; ++i )
            {
                h.u( indices[i] );
                h.d( dists[i] );
            }
        }

        sec.add( h );
    }

    sec.print();
}


int main( int argc, char** argv )
{
    if( argc == 3 && std::string( argv[1] ) == "--dump" )
        g_dump = fopen( argv[2], "w" );

    sectionSort();
    sectionPartialSort();
    sectionHeap();
    sectionPacked<2>( "packed_rtree_2d", 606, 400 );
    sectionPacked<3>( "packed_rtree_3d", 607, 150 );
    sectionDynamic<2>( "dynamic_rtree_2d", 708, 300 );
    sectionDynamic<3>( "dynamic_rtree_3d", 709, 150 );
    sectionDelaunator();
    sectionNanoflann();

    if( g_dump )
        fclose( g_dump );

    return 0;
}
