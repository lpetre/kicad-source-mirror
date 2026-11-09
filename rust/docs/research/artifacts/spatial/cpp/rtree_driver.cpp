// Research driver: exercises KiCad's own KIRTREE::PACKED_RTREE (2D) and KIRTREE::DYNAMIC_RTREE
// (2D and 3D; BulkLoad, Insert, Remove) and prints the *visit order* of every query.
//
// Input: <ncases> then per case:
//   <dim> <n> <nq>
//   n lines:  min[0..dim) max[0..dim)
//   nq lines: min[0..dim) max[0..dim)
// Output per case:
//   case <idx> dim <dim>
//   PI <ids in packed iteration order>            (dim == 2 only)
//   PQ <k> <ids in visit order>   x nq            (dim == 2 only)
//   BQ <k> <ids ...>              x nq            DYNAMIC_RTREE after BulkLoad
//   IQ <k> <ids ...>              x nq            DYNAMIC_RTREE after n x Insert
//   RQ <k> <ids ...>              x nq            ... after removing every item with odd id
#include <geometry/rtree/dynamic_rtree.h>
#include <geometry/rtree/packed_rtree.h>

#include <cstdio>
#include <vector>

template <int DIM>
static void runCase( FILE* in, FILE* out, size_t n, size_t nq )
{
    struct BOX
    {
        int min[DIM];
        int max[DIM];
    };

    std::vector<BOX> items( n ), queries( nq );

    auto readBoxes = [&]( std::vector<BOX>& v )
    {
        for( BOX& b : v )
        {
            for( int d = 0; d < DIM; ++d )
            {
                if( fscanf( in, "%d", &b.min[d] ) != 1 )
                    exit( 2 );
            }

            for( int d = 0; d < DIM; ++d )
            {
                if( fscanf( in, "%d", &b.max[d] ) != 1 )
                    exit( 2 );
            }
        }
    };

    readBoxes( items );
    readBoxes( queries );

    auto runQueries = [&]( const char* tag, auto& tree )
    {
        for( const BOX& q : queries )
        {
            std::vector<int> hits;
            auto             visitor = [&]( int id ) -> bool
            {
                hits.push_back( id );
                return true;
            };

            tree.Search( q.min, q.max, visitor );
            fprintf( out, "%s %zu", tag, hits.size() );

            for( int id : hits )
                fprintf( out, " %d", id );

            fprintf( out, "\n" );
        }
    };

    if( DIM == 2 )
    {
        typename KIRTREE::PACKED_RTREE<int, int, DIM>::Builder builder;

        for( size_t i = 0; i < n; ++i )
            builder.Add( items[i].min, items[i].max, static_cast<int>( i ) );

        KIRTREE::PACKED_RTREE<int, int, DIM> tree = builder.Build();

        fprintf( out, "PI" );

        for( int id : tree )
            fprintf( out, " %d", id );

        fprintf( out, "\n" );
        runQueries( "PQ", tree );
    }

    {
        using TREE = KIRTREE::DYNAMIC_RTREE<int, int, DIM>;
        TREE                                  tree;
        std::vector<typename TREE::BULK_ENTRY> entries( n );

        for( size_t i = 0; i < n; ++i )
        {
            for( int d = 0; d < DIM; ++d )
            {
                entries[i].min[d] = items[i].min[d];
                entries[i].max[d] = items[i].max[d];
            }

            entries[i].data = static_cast<int>( i );
        }

        tree.BulkLoad( entries );
        runQueries( "BQ", tree );
    }

    {
        KIRTREE::DYNAMIC_RTREE<int, int, DIM> tree;

        for( size_t i = 0; i < n; ++i )
            tree.Insert( items[i].min, items[i].max, static_cast<int>( i ) );

        runQueries( "IQ", tree );

        for( size_t i = 1; i < n; i += 2 )
        {
            if( !tree.Remove( items[i].min, items[i].max, static_cast<int>( i ) ) )
                fprintf( out, "REMOVE-FAILED %zu\n", i );
        }

        runQueries( "RQ", tree );
    }
}


int main( int argc, char** argv )
{
    if( argc < 3 )
        return 2;

    FILE* in = fopen( argv[1], "r" );
    FILE* out = fopen( argv[2], "w" );

    if( !in || !out )
        return 2;

    size_t ncases = 0;

    if( fscanf( in, "%zu", &ncases ) != 1 )
        return 2;

    for( size_t c = 0; c < ncases; ++c )
    {
        int    dim = 0;
        size_t n = 0, nq = 0;

        if( fscanf( in, "%d %zu %zu", &dim, &n, &nq ) != 3 )
            return 2;

        fprintf( out, "case %zu dim %d\n", c, dim );

        if( dim == 2 )
            runCase<2>( in, out, n, nq );
        else
            runCase<3>( in, out, n, nq );
    }

    fclose( in );
    fclose( out );
    return 0;
}
