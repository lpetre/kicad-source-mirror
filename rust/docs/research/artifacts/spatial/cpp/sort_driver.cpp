// Research driver: std::sort / std::partial_sort (libstdc++) tie order.
// Input: <ncases>, per case: <mode S|P> <n> <m> then n integer keys.
// Output per case: the element ids (original positions) after sorting pairs (key, id) by key only.
#include <algorithm>
#include <cstdio>
#include <utility>
#include <vector>

int main( int argc, char** argv )
{
    if( argc < 3 )
        return 2;

    FILE* in = fopen( argv[1], "r" );
    FILE* out = fopen( argv[2], "w" );
    size_t ncases = 0;

    if( !in || !out || fscanf( in, "%zu", &ncases ) != 1 )
        return 2;

    for( size_t c = 0; c < ncases; ++c )
    {
        char   mode = 0;
        size_t n = 0, m = 0;

        if( fscanf( in, " %c %zu %zu", &mode, &n, &m ) != 3 )
            return 2;

        std::vector<std::pair<long long, int>> v( n );

        for( size_t i = 0; i < n; ++i )
        {
            if( fscanf( in, "%lld", &v[i].first ) != 1 )
                return 2;

            v[i].second = static_cast<int>( i );
        }

        auto less = []( const std::pair<long long, int>& a, const std::pair<long long, int>& b )
        {
            return a.first < b.first;
        };

        if( mode == 'S' )
            std::sort( v.begin(), v.end(), less );
        else
            std::partial_sort( v.begin(), v.begin() + m, v.end(), less );

        for( size_t i = 0; i < n; ++i )
            fprintf( out, "%d ", v[i].second );

        fprintf( out, "\n" );
    }

    fclose( in );
    fclose( out );
    return 0;
}
