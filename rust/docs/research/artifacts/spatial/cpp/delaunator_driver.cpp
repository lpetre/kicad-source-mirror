// Research driver: runs KiCad's bundled thirdparty/delaunator on a file of point sets and
// prints triangles / halfedges / hull for comparison with Rust candidates.
//
// Input:  <ncases>\n then per case: <n>\n followed by n lines "x y" (decimal doubles)
// Output: per case
//   case <idx>
//   EXC <what>                      (if the constructor threw)
//   T <count> <i> <i> ...
//   H <count> <i> <i> ...           (-1 for INVALID_INDEX)
//   HULL <count> <i> ...            (walk hull_start via hull_next)
#include "delaunator.hpp"

#include <cstdio>
#include <cstdlib>
#include <exception>
#include <vector>

int main( int argc, char** argv )
{
    if( argc < 3 )
    {
        fprintf( stderr, "usage: %s cases.txt out.txt\n", argv[0] );
        return 2;
    }

    FILE* in = fopen( argv[1], "r" );
    FILE* out = fopen( argv[2], "w" );

    if( !in || !out )
        return 2;

    size_t ncases = 0;

    if( fscanf( in, "%zu", &ncases ) != 1 )
        return 2;

    for( size_t c = 0; c < ncases; ++c )
    {
        size_t n = 0;

        if( fscanf( in, "%zu", &n ) != 1 )
            return 2;

        std::vector<double> coords( 2 * n );

        for( size_t i = 0; i < 2 * n; ++i )
        {
            if( fscanf( in, "%lf", &coords[i] ) != 1 )
                return 2;
        }

        fprintf( out, "case %zu\n", c );

        try
        {
            delaunator::Delaunator d( coords );

            fprintf( out, "T %zu", d.triangles.size() );

            for( size_t t : d.triangles )
                fprintf( out, " %zu", t );

            fprintf( out, "\nH %zu", d.halfedges.size() );

            for( size_t h : d.halfedges )
            {
                if( h == delaunator::INVALID_INDEX )
                    fprintf( out, " -1" );
                else
                    fprintf( out, " %zu", h );
            }

            std::vector<size_t> hull;
            size_t              e = d.hull_start;

            do
            {
                hull.push_back( e );
                e = d.hull_next[e];
            } while( e != d.hull_start && hull.size() <= n );

            fprintf( out, "\nHULL %zu", hull.size() );

            for( size_t h : hull )
                fprintf( out, " %zu", h );

            fprintf( out, "\n" );
        }
        catch( const std::exception& ex )
        {
            fprintf( out, "EXC %s\n", ex.what() );
        }
    }

    fclose( in );
    fclose( out );
    return 0;
}
